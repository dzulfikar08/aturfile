//! In-app updates (see `af_update`): a check at launch at most once a day
//! (Settings `auto_update`) and from the palette or the app menu, a
//! background download, and a status-bar notice once the new build is
//! ready. The swap happens on "Restart to Update" or when aturfile quits.
//!
//! The Mac App Store build never updates itself (App Review guideline
//! 2.4.5): none of this runs there and the commands are hidden.

use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use af_update::http::Http;
use af_update::verify::SystemVerifier;
use af_update::{Config, Error, InstallKind, Staged, State};
use gpui_kit::*;

use super::Workspace;

/// The staged update applied at quit; kept outside the workspace so the
/// quit hook in `main` can reach it.
static ON_QUIT: Mutex<Option<Staged>> = Mutex::new(None);

#[derive(Default)]
pub(crate) enum UpdateStatus {
    #[default]
    Idle,
    Checking,
    Downloading {
        version: String,
    },
    /// Downloaded and verified; swapped in on restart or quit.
    Ready(Staged),
    /// A newer build aturfile can't install itself (not an app bundle, a
    /// read-only location, a failed download): offer the release page.
    Available {
        version: String,
        url: String,
    },
}

#[derive(Default)]
pub(crate) struct Updates {
    pub(crate) status: UpdateStatus,
    /// Bytes downloaded and expected, written by the download thread.
    progress: Arc<(AtomicU64, AtomicU64)>,
    /// The running check was asked for: report every outcome.
    manual: bool,
}

/// Whether this build updates itself at all.
pub(crate) fn enabled() -> bool {
    !af_store::STORE_BUILD
}

fn state_path() -> PathBuf {
    af_mac::app_data_dir().join("update.json")
}

fn load_state() -> State {
    State::load_from(&state_path())
}

fn save_state(state: &State) {
    if let Err(e) = state.save_to(&state_path()) {
        eprintln!("aturfile: saving update state: {e}");
    }
}

fn set_on_quit(staged: Option<Staged>) {
    if let Ok(mut slot) = ON_QUIT.lock() {
        *slot = staged;
    }
}

/// aturfile is quitting: put a downloaded update in place for next time.
pub fn apply_on_quit() {
    let staged = ON_QUIT.lock().ok().and_then(|mut s| s.take());
    if let Some(staged) = staged
        && let Err(e) = apply_staged(&staged, false)
    {
        eprintln!("aturfile: update on quit failed (old build kept): {e}");
    }
}

impl Updates {
    /// The clickable status-bar text, if there is something to say.
    pub(crate) fn status_text(&self) -> Option<String> {
        match &self.status {
            UpdateStatus::Ready(s) => Some(format!(
                "aturfile {} is ready: restart to update",
                s.version
            )),
            UpdateStatus::Available { version, .. } => {
                Some(format!("aturfile {version} is available: download"))
            }
            UpdateStatus::Downloading { version } if self.manual => {
                let (done, total) = (
                    self.progress.0.load(Ordering::Relaxed),
                    self.progress.1.load(Ordering::Relaxed),
                );
                let pct = (done * 100).checked_div(total).unwrap_or(0);
                Some(format!("Downloading aturfile {version}\u{2026} {pct}%"))
            }
            UpdateStatus::Checking if self.manual => {
                Some("Checking for updates\u{2026}".to_string())
            }
            _ => None,
        }
    }

    /// One line for the About sheet.
    pub(crate) fn about_line(&self, auto: bool) -> String {
        match &self.status {
            UpdateStatus::Ready(s) => format!("Version {} is ready: restart to update", s.version),
            UpdateStatus::Available { version, .. } => format!("Version {version} is available"),
            UpdateStatus::Downloading { version } => format!("Downloading version {version}"),
            UpdateStatus::Checking => "Checking for updates".to_string(),
            UpdateStatus::Idle if auto => "Checks for updates daily".to_string(),
            UpdateStatus::Idle => "Automatic update checks are off".to_string(),
        }
    }
}

impl Workspace {
    /// At launch: pick up an update staged by an earlier run, then check if
    /// a day has passed since the last check.
    pub(super) fn init_updates(&mut self, cx: &mut Context<Self>) {
        if !enabled() {
            return;
        }
        let mut state = load_state();
        af_update::clean_leftovers(&InstallKind::detect(), state.staged.as_ref());
        if let Some(staged) = state.staged.take() {
            if af_update::staged_is_current(&staged, &af_update::running_version()) {
                set_on_quit(Some(staged.clone()));
                self.updates.status = UpdateStatus::Ready(staged);
                return;
            }
            // Installed by hand, or older than this build.
            af_update::discard(&staged);
            save_state(&state);
        }
        if self.settings.auto_update
            && af_update::state::due(state.last_check, af_update::state::now())
        {
            self.check_for_updates(false, cx);
        }
    }

    /// Check now (`manual`: from the palette or menu); download in the
    /// background when there is something newer.
    pub(crate) fn check_for_updates(&mut self, manual: bool, cx: &mut Context<Self>) {
        if !enabled() {
            if manual {
                self.set_notice(
                    "aturfile from the App Store is updated by the App Store",
                    cx,
                );
            }
            return;
        }
        match &self.updates.status {
            UpdateStatus::Checking | UpdateStatus::Downloading { .. } => {
                if manual {
                    self.updates.manual = true;
                    cx.notify();
                }
                return;
            }
            UpdateStatus::Ready(s) => {
                if manual {
                    let text = format!("aturfile {} is ready: choose Restart to Update", s.version);
                    self.set_notice(text, cx);
                }
                return;
            }
            _ => {}
        }
        let mut state = load_state();
        state.last_check = Some(af_update::state::now());
        save_state(&state);

        let cfg = Config::for_this_build();
        self.updates.manual = manual;
        self.updates.status = UpdateStatus::Checking;
        self.updates.progress.0.store(0, Ordering::Relaxed);
        self.updates.progress.1.store(0, Ordering::Relaxed);
        cx.notify();
        let progress = self.updates.progress.clone();

        cx.spawn(async move |this, cx| {
            let check_cfg = cfg.clone();
            let checked = cx
                .background_executor()
                .spawn(async move { af_update::check(&check_cfg, &Http::new()) })
                .await;
            let available = match checked {
                Ok(Some(a)) => a,
                Ok(None) => {
                    let _ = this.update(cx, |ws, cx| {
                        ws.updates.status = UpdateStatus::Idle;
                        if ws.updates.manual {
                            let me = af_update::current_version();
                            ws.set_notice(format!("aturfile {me} is up to date"), cx);
                        }
                        cx.notify();
                    });
                    return;
                }
                Err(e) => {
                    let _ = this.update(cx, |ws, cx| {
                        ws.updates.status = UpdateStatus::Idle;
                        if ws.updates.manual {
                            ws.set_notice(format!("Update check failed: {e}"), cx);
                        }
                        cx.notify();
                    });
                    return;
                }
            };
            let version = available.version.to_string();
            let url = available.manifest.notes_url.clone();
            if available.asset.is_none() {
                let _ = this.update(cx, |ws, cx| {
                    ws.updates.status = UpdateStatus::Available { version, url };
                    cx.notify();
                });
                return;
            }
            let _ = this.update(cx, |ws, cx| {
                ws.updates.status = UpdateStatus::Downloading {
                    version: version.clone(),
                };
                cx.notify();
            });
            // Redraw the percentage while a manual download runs.
            let ticker = this.clone();
            cx.spawn(async move |cx| {
                loop {
                    cx.background_executor()
                        .timer(Duration::from_millis(500))
                        .await;
                    let going = ticker.update(cx, |ws, cx| {
                        let going = matches!(ws.updates.status, UpdateStatus::Downloading { .. });
                        if going && ws.updates.manual {
                            cx.notify();
                        }
                        going
                    });
                    if !matches!(going, Ok(true)) {
                        break;
                    }
                }
            })
            .detach();

            let staged = cx
                .background_executor()
                .spawn(async move {
                    af_update::stage(
                        &cfg,
                        &available,
                        &Http::new(),
                        &SystemVerifier,
                        &mut |done, total| {
                            progress.0.store(done, Ordering::Relaxed);
                            progress.1.store(total, Ordering::Relaxed);
                        },
                    )
                })
                .await;
            let _ = this.update(cx, |ws, cx| {
                match staged {
                    Ok(staged) => {
                        let mut state = load_state();
                        state.staged = Some(staged.clone());
                        save_state(&state);
                        set_on_quit(Some(staged.clone()));
                        ws.updates.status = UpdateStatus::Ready(staged);
                    }
                    Err(e) => {
                        if ws.updates.manual || matches!(e, Error::NotWritable(_)) {
                            ws.set_notice(format!("Couldn't install aturfile {version}: {e}"), cx);
                        }
                        ws.updates.status = UpdateStatus::Available { version, url };
                    }
                }
                cx.notify();
            });
        })
        .detach();
    }

    /// The status-bar notice was clicked.
    pub(super) fn update_clicked(&mut self, cx: &mut Context<Self>) {
        match &self.updates.status {
            UpdateStatus::Ready(_) => self.restart_to_update(cx),
            UpdateStatus::Available { url, .. } => cx.open_url(url),
            _ => {}
        }
    }

    /// Swap the downloaded build in and start it, then quit.
    pub(crate) fn restart_to_update(&mut self, cx: &mut Context<Self>) {
        if !enabled() {
            return self.set_notice(
                "aturfile from the App Store is updated by the App Store",
                cx,
            );
        }
        let UpdateStatus::Ready(staged) = std::mem::take(&mut self.updates.status) else {
            return self.set_notice(
                "No update is ready. Use Check for Updates\u{2026} first.",
                cx,
            );
        };
        set_on_quit(None);
        match apply_staged(&staged, true) {
            Ok(()) => cx.quit(),
            Err(e) => {
                self.set_notice(format!("Update failed, nothing changed: {e}"), cx);
                self.updates.status = UpdateStatus::Available {
                    version: staged.version.clone(),
                    url: staged.notes_url.clone(),
                };
                cx.notify();
            }
        }
    }

    /// Settings: turn the daily check on or off.
    pub(crate) fn toggle_auto_update(&mut self, cx: &mut Context<Self>) {
        self.settings.auto_update = !self.settings.auto_update;
        self.save_settings();
        let text = if self.settings.auto_update {
            "Automatic update checks on"
        } else {
            "Automatic update checks off"
        };
        self.set_notice(text, cx);
    }
}

/// Swap in (and with `relaunch` start) and forget the staged update; a
/// failed swap leaves the old build and drops the download, so the next
/// check starts clean.
fn apply_staged(staged: &Staged, relaunch: bool) -> Result<(), Error> {
    let result = if relaunch {
        af_update::apply_and_relaunch(staged)
    } else {
        af_update::apply(staged)
    };
    if result.is_err() {
        af_update::discard(staged);
    }
    let mut state = load_state();
    state.staged = None;
    save_state(&state);
    result
}
