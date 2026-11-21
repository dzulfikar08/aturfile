//! Live folder updates: every folder on screen (each tab's folder and the
//! Miller columns of the active tab) is watched with `notify` (FSEvents on
//! macOS). Changes are debounced, then the folder is re-listed in the
//! background with the selection kept by path. `/Volumes` is watched too,
//! so mounted and ejected disks show up at once.

use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::time::Duration;

use gpui_kit::*;
use notify::{RecursiveMode, Watcher};

use super::{Select, Workspace};
use crate::tab::Page;

/// Changes within this window are handled together.
const DEBOUNCE: Duration = Duration::from_millis(150);

pub(crate) struct Watch {
    watcher: Option<notify::RecommendedWatcher>,
    watched: HashSet<PathBuf>,
    _task: Task<()>,
}

const VOLUMES: &str = "/Volumes";

impl Watch {
    pub fn new(cx: &mut Context<Workspace>) -> Self {
        let (tx, rx) = async_channel::unbounded::<PathBuf>();
        let watcher = notify::recommended_watcher(move |res: notify::Result<notify::Event>| {
            if let Ok(event) = res {
                for path in event.paths {
                    let _ = tx.send_blocking(path);
                }
            }
        })
        .ok();
        let task = cx.spawn(async move |this, cx| {
            while let Ok(first) = rx.recv().await {
                let mut changed: HashSet<PathBuf> = HashSet::from([first]);
                cx.background_executor().timer(DEBOUNCE).await;
                while let Ok(p) = rx.try_recv() {
                    changed.insert(p);
                }
                if this
                    .update(cx, |ws, cx| ws.on_fs_changes(changed, cx))
                    .is_err()
                {
                    break;
                }
            }
        });
        Self {
            watcher,
            watched: HashSet::new(),
            _task: task,
        }
    }
}

impl Workspace {
    /// Folders on screen now.
    fn visible_folders(&self) -> HashSet<PathBuf> {
        let mut dirs: HashSet<PathBuf> = self
            .tabs
            .iter()
            .filter(|t| t.page == Page::Files)
            .map(|t| t.cwd.clone())
            .collect();
        dirs.extend(self.tab().listings.keys().cloned());
        if cfg!(target_os = "macos") {
            dirs.insert(PathBuf::from(VOLUMES));
        }
        dirs
    }

    /// Watch what is on screen and nothing else (after each render).
    pub(super) fn sync_watches(&mut self) {
        let want = self.visible_folders();
        let w = &mut self.watch;
        let Some(watcher) = w.watcher.as_mut() else {
            return;
        };
        if want == w.watched {
            return;
        }
        for gone in w.watched.difference(&want) {
            let _ = watcher.unwatch(gone);
        }
        for new in want.difference(&w.watched) {
            let _ = watcher.watch(new, RecursiveMode::NonRecursive);
        }
        w.watched = want;
    }

    fn on_fs_changes(&mut self, changed: HashSet<PathBuf>, cx: &mut Context<Self>) {
        // A change names the item; its folder is what to re-list. FSEvents
        // may also name the folder itself.
        let mut dirs: HashSet<PathBuf> = HashSet::new();
        for p in &changed {
            if self.watch.watched.contains(p) {
                dirs.insert(p.clone());
            }
            if let Some(parent) = p.parent() {
                dirs.insert(parent.to_path_buf());
            }
        }
        if dirs.iter().any(|d| d == Path::new(VOLUMES)) {
            self.refresh_volumes(cx);
        }
        for ix in 0..self.tabs.len() {
            let tab = &mut self.tabs[ix];
            tab.listings.retain(|dir, _| !dirs.contains(dir));
            if tab.page == Page::Files && dirs.contains(&tab.cwd) {
                let keep =
                    Select::Refresh(tab.selected_paths(), tab.current().map(|e| e.path.clone()));
                let dir = tab.cwd.clone();
                self.load(ix, dir, keep, cx);
            }
        }
        if changed.iter().any(|p| self.preview_shows(p)) {
            self.invalidate_preview();
        }
        cx.notify();
    }

    /// Re-read the mounted volumes now.
    pub(crate) fn refresh_volumes(&mut self, cx: &mut Context<Self>) {
        cx.spawn(async move |this, cx| {
            let vols = cx
                .background_executor()
                .spawn(async { af_mac::volumes() })
                .await;
            let _ = this.update(cx, |ws, cx| {
                if ws.volumes != vols {
                    ws.volumes = vols;
                    cx.notify();
                }
            });
        })
        .detach();
    }
}
