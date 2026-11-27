//! File operations with undo: New Folder, Rename (inline), Duplicate,
//! Copy/Cut/Paste, Move to Trash. Long copies run on a background thread
//! with progress in the status bar. The logic lives in `af_fs::ops`;
//! trashing goes through the cleanup engine's protected-path guard.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use af_fs::ops::{self, Op, UndoStack};
use af_ui::components::meter;
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::component::h_flex;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{Select, Workspace};
use crate::actions::*;
use crate::tab::Page;

pub(crate) struct Renaming {
    pub path: PathBuf,
}

struct Clip {
    paths: Vec<PathBuf>,
    cut: bool,
}

struct Job {
    label: String,
    done: Arc<AtomicU64>,
    total: u64,
    _task: Task<()>,
}

pub struct FileOps {
    pub(crate) renaming: Option<Renaming>,
    pub(crate) rename_input: Entity<InputState>,
    clip: Option<Clip>,
    history: UndoStack<Op>,
    job: Option<Job>,
    _subscription: Subscription,
}

impl FileOps {
    pub fn new(window: &mut Window, cx: &mut Context<Workspace>) -> Self {
        let rename_input = cx.new(|cx| InputState::new(window, cx));
        let sub = cx.subscribe_in(&rename_input, window, Workspace::on_rename_input);
        Self {
            renaming: None,
            rename_input,
            clip: None,
            history: UndoStack::new(100),
            job: None,
            _subscription: sub,
        }
    }

    pub(crate) fn can_paste(&self) -> bool {
        self.clip.as_ref().is_some_and(|c| !c.paths.is_empty())
    }

    fn busy(&self) -> bool {
        self.job.is_some()
    }
}

/// Move `path` to the Trash unless the cleanup engine protects it. Returns
/// where it went, for undo.
fn guarded_trash(path: &Path) -> std::io::Result<Option<PathBuf>> {
    af_clean::cleanup::check_protected(path).map_err(std::io::Error::other)?;
    af_mac::trash_located(path)
}

fn parent_dirs(paths: &[PathBuf]) -> Vec<PathBuf> {
    let mut dirs: Vec<PathBuf> = paths
        .iter()
        .filter_map(|p| p.parent().map(Path::to_path_buf))
        .collect();
    dirs.sort();
    dirs.dedup();
    dirs
}

fn quoted(path: &Path) -> String {
    format!("\u{201c}{}\u{201d}", crate::tab::folder_name(path))
}

impl Workspace {
    /// File commands act when the file list has the keyboard.
    fn ops_allowed(&self, window: &Window) -> bool {
        self.list_focused(window) && self.tab().page == Page::Files
    }

    // ---- rename -------------------------------------------------------------

    pub(crate) fn begin_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let selected = self.tab().selected();
        let [entry] = selected.as_slice() else {
            return;
        };
        let (path, name, is_dir) = (entry.path.clone(), entry.name.clone(), entry.is_dir);
        let stem = ops::split_name(&name, is_dir).0.len();
        self.ops.renaming = Some(Renaming { path });
        self.ops.rename_input.update(cx, |input, cx| {
            input.set_value(name, window, cx);
            input.focus(window, cx);
            input.set_selected_range(0..stem, cx);
        });
        self.scroll_to_cursor();
        cx.notify();
    }

    pub(crate) fn cancel_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.ops.renaming.take().is_some() {
            self.focus_list(window, cx);
            cx.notify();
        }
    }

    fn commit_rename(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(r) = self.ops.renaming.take() else {
            return;
        };
        let typed = self.ops.rename_input.read(cx).value().to_string();
        self.focus_list(window, cx);
        let name = match ops::validate_name(&typed) {
            Ok(n) => n.to_owned(),
            Err(e) => return self.set_notice(e, cx),
        };
        let Some(dir) = r.path.parent().map(Path::to_path_buf) else {
            return;
        };
        let to = dir.join(&name);
        if to == r.path {
            cx.notify();
            return;
        }
        // Changing only the case of a name is fine on case-insensitive disks.
        let case_only = to
            .to_string_lossy()
            .eq_ignore_ascii_case(&r.path.to_string_lossy());
        if to.symlink_metadata().is_ok() && !case_only {
            return self.set_notice(format!("\u{201c}{name}\u{201d} already exists here"), cx);
        }
        match std::fs::rename(&r.path, &to) {
            Ok(()) => {
                self.ops.history.push(Op::Renamed {
                    from: r.path.clone(),
                    to: to.clone(),
                });
                self.notice = Some(format!("Renamed to \u{201c}{name}\u{201d}").into());
                self.invalidate_preview();
                self.reload_selecting(&dir, vec![to], cx);
            }
            Err(e) => self.set_notice(format!("Couldn't rename: {e}"), cx),
        }
    }

    fn on_rename_input(
        &mut self,
        _: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => self.commit_rename(window, cx),
            InputEvent::Blur if self.ops.renaming.is_some() => {
                self.commit_rename(window, cx);
            }
            _ => {}
        }
    }

    /// Reload tabs on `dir`, selecting `paths` in the active one.
    fn reload_selecting(&mut self, dir: &Path, paths: Vec<PathBuf>, cx: &mut Context<Self>) {
        let cursor = paths.first().cloned();
        for ix in 0..self.tabs.len() {
            if self.tabs[ix].cwd != dir {
                continue;
            }
            let select = if ix == self.active {
                Select::Keep(paths.clone(), cursor.clone())
            } else {
                let tab = &self.tabs[ix];
                Select::Keep(tab.selected_paths(), tab.current().map(|e| e.path.clone()))
            };
            self.load(ix, dir.to_path_buf(), select, cx);
        }
        cx.notify();
    }

    // ---- new folder, duplicate ------------------------------------------------

    pub(crate) fn new_folder(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().page != Page::Files {
            return;
        }
        let dir = self.tab().cwd.clone();
        let path = ops::unique_name(&dir, "untitled folder", true);
        if let Err(e) = std::fs::create_dir(&path) {
            return self.set_notice(format!("Couldn't create a folder: {e}"), cx);
        }
        self.ops.history.push(Op::Created(path.clone()));
        // Show it at once and type its name; no need to wait for a re-read.
        let show_hidden = self.show_hidden;
        if let Some(entry) = af_fs::entry_for(&path) {
            let tab = self.tab_mut();
            tab.query.clear();
            tab.entries.push(entry);
            tab.sort_entries();
            tab.refilter(show_hidden);
            tab.reselect(std::slice::from_ref(&path), Some(&path));
        }
        self.begin_rename(window, cx);
    }

    pub(crate) fn duplicate(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let pairs: Vec<(PathBuf, PathBuf)> = self
            .tab()
            .selected()
            .iter()
            .filter_map(|e| {
                let dir = e.path.parent()?;
                Some((e.path.clone(), ops::unique_name(dir, &e.name, e.is_dir)))
            })
            .collect();
        if !pairs.is_empty() {
            self.transfer(pairs, false, cx);
        }
    }

    // ---- clipboard -----------------------------------------------------------

    pub(crate) fn copy_selection(&mut self, cut: bool, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.tab().selected_paths();
        if paths.is_empty() {
            return;
        }
        let text = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        let n = paths.len();
        self.ops.clip = Some(Clip { paths, cut });
        let what = if n == 1 {
            "1 item".to_string()
        } else {
            format!("{n} items")
        };
        self.set_notice(
            if cut {
                format!("Cut {what}: paste to move")
            } else {
                format!("Copied {what}")
            },
            cx,
        );
    }

    /// Paste into the current folder. `force_move` (Cmd-Alt-V) moves even
    /// copied items, like Finder.
    pub(crate) fn paste(&mut self, force_move: bool, _: &mut Window, cx: &mut Context<Self>) {
        let Some(clip) = self.ops.clip.as_ref() else {
            return;
        };
        let dir = self.tab().cwd.clone();
        let moving = clip.cut || force_move;
        let paths = clip.paths.clone();
        if self.put_into(&dir, &paths, moving, cx) && moving {
            self.ops.clip = None;
        }
    }

    /// Copy or move `paths` into `dir` (paste, drop). Items already in `dir`
    /// are skipped when moving. Returns false when refused.
    pub(crate) fn put_into(
        &mut self,
        dir: &Path,
        paths: &[PathBuf],
        moving: bool,
        cx: &mut Context<Self>,
    ) -> bool {
        let mut pairs = Vec::new();
        for src in paths {
            let Some(name) = src.file_name().map(|n| n.to_string_lossy().into_owned()) else {
                continue;
            };
            if moving && src.parent() == Some(dir) {
                continue; // already here
            }
            if dir.starts_with(src) {
                self.set_notice(format!("Can't put {} inside itself", quoted(src)), cx);
                return false;
            }
            let is_dir = src.is_dir();
            pairs.push((src.clone(), ops::unique_name(dir, &name, is_dir)));
        }
        if moving {
            for (src, _) in &pairs {
                if let Err(e) = af_clean::cleanup::check_protected(src) {
                    self.set_notice(format!("Not moved: {e}"), cx);
                    return false;
                }
            }
        }
        if !pairs.is_empty() {
            self.transfer(pairs, moving, cx);
        }
        true
    }

    /// Copy or move `pairs` on a background thread, then record the op and
    /// select the results.
    fn transfer(&mut self, pairs: Vec<(PathBuf, PathBuf)>, moving: bool, cx: &mut Context<Self>) {
        if self.ops.busy() {
            return self.set_notice("Wait for the current copy to finish", cx);
        }
        let n = pairs.len();
        let label = format!(
            "{} {} item{}",
            if moving { "Moving" } else { "Copying" },
            n,
            if n == 1 { "" } else { "s" }
        );
        let total: u64 = pairs.iter().map(|(s, _)| ops::total_bytes(s)).sum();
        let done = Arc::new(AtomicU64::new(0));
        let counter = done.clone();
        let work_pairs = pairs.clone();
        let work = cx.background_executor().spawn(async move {
            let mut finished = Vec::new();
            let mut error = None;
            let mut progress = |b: u64| {
                counter.fetch_add(b, Ordering::Relaxed);
            };
            for (src, dst) in work_pairs {
                let r = if moving {
                    ops::move_path(&src, &dst, &mut progress)
                } else {
                    ops::copy_path(&src, &dst, &mut progress)
                };
                match r {
                    Ok(()) => finished.push((src, dst)),
                    Err(e) => {
                        error = Some(format!("{}: {e}", quoted(&src)));
                        break;
                    }
                }
            }
            (finished, error)
        });
        let task = cx.spawn(async move |this, cx| {
            let (finished, error) = work.await;
            let _ = this.update(cx, |ws, cx| {
                ws.ops.job = None;
                if !finished.is_empty() {
                    let op = if moving {
                        Op::Moved(finished.clone())
                    } else {
                        Op::Copied(finished.clone())
                    };
                    ws.ops.history.push(op);
                }
                ws.notice = Some(match error {
                    Some(e) => format!("Stopped: {e}").into(),
                    None => format!(
                        "{} {}",
                        if moving { "Moved" } else { "Copied" },
                        crate::format::items(finished.len())
                    )
                    .into(),
                });
                let mut dirs =
                    parent_dirs(&finished.iter().map(|(s, _)| s.clone()).collect::<Vec<_>>());
                let results: Vec<PathBuf> = finished.iter().map(|(_, d)| d.clone()).collect();
                let dest_dirs = parent_dirs(&results);
                for d in &dest_dirs {
                    ws.reload_selecting(d, results.clone(), cx);
                }
                dirs.retain(|d| !dest_dirs.contains(d));
                ws.reload_dirs(&dirs, cx);
                cx.notify();
            });
        });
        // Repaint the progress while it runs.
        let poll = cx.spawn(async move |this, cx| {
            loop {
                cx.background_executor()
                    .timer(Duration::from_millis(150))
                    .await;
                let running = this
                    .update(cx, |ws, cx| {
                        cx.notify();
                        ws.ops.busy()
                    })
                    .unwrap_or(false);
                if !running {
                    break;
                }
            }
        });
        poll.detach();
        self.ops.job = Some(Job {
            label,
            done,
            total,
            _task: task,
        });
        cx.notify();
    }

    // ---- trash ----------------------------------------------------------------

    pub(crate) fn trash_selected(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.tab().selected_paths();
        if paths.is_empty() {
            return;
        }
        let work = cx.background_executor().spawn(async move {
            let mut moved = Vec::new();
            let mut failed = Vec::new();
            for p in &paths {
                match guarded_trash(p) {
                    Ok(Some(at)) => moved.push((p.clone(), at)),
                    Ok(None) => moved.push((p.clone(), PathBuf::new())),
                    Err(e) => failed.push(format!("{}: {e}", quoted(p))),
                }
            }
            (moved, failed)
        });
        cx.spawn(async move |this, cx| {
            let (moved, failed) = work.await;
            let _ = this.update(cx, |ws, cx| {
                let undoable: Vec<_> = moved
                    .iter()
                    .filter(|(_, at)| !at.as_os_str().is_empty())
                    .cloned()
                    .collect();
                if !undoable.is_empty() {
                    ws.ops.history.push(Op::Trashed(undoable));
                }
                ws.notice = Some(match (moved.len(), failed.first()) {
                    (0, Some(e)) => format!("Not moved: {e}").into(),
                    (n, Some(e)) => format!("Moved {n} to Trash; not moved: {e}").into(),
                    (1, None) => format!("Moved {} to Trash", quoted(&moved[0].0)).into(),
                    (n, None) => format!("Moved {n} items to Trash").into(),
                });
                let origins: Vec<PathBuf> = moved.iter().map(|(p, _)| p.clone()).collect();
                ws.reload_dirs(&parent_dirs(&origins), cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ---- undo -----------------------------------------------------------------

    pub(crate) fn undo(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.ops.busy() {
            return;
        }
        let Some(op) = self.ops.history.take_undo() else {
            return self.set_notice("Nothing to undo", cx);
        };
        let work_op = op.clone();
        let work = cx
            .background_executor()
            .spawn(async move { ops::undo(&work_op, &guarded_trash) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(()) => {
                        ws.notice = Some(format!("Undid {}", op.label()).into());
                        ws.ops.history.undone(op.clone());
                    }
                    Err(e) => ws.notice = Some(format!("Couldn't undo {}: {e}", op.label()).into()),
                }
                ws.invalidate_preview();
                ws.reload_dirs(&parent_dirs(&op.paths()), cx);
                cx.notify();
            });
        })
        .detach();
    }

    pub(crate) fn redo(&mut self, _: &mut Window, cx: &mut Context<Self>) {
        if self.ops.busy() {
            return;
        }
        let Some(op) = self.ops.history.take_redo() else {
            return self.set_notice("Nothing to redo", cx);
        };
        let work_op = op.clone();
        let work = cx
            .background_executor()
            .spawn(async move { ops::redo(&work_op, &guarded_trash) });
        cx.spawn(async move |this, cx| {
            let result = work.await;
            let _ = this.update(cx, |ws, cx| {
                match result {
                    Ok(now) => {
                        ws.notice = Some(format!("Redid {}", now.label()).into());
                        ws.ops.history.redone(now);
                    }
                    Err(e) => ws.notice = Some(format!("Couldn't redo {}: {e}", op.label()).into()),
                }
                ws.invalidate_preview();
                ws.reload_dirs(&parent_dirs(&op.paths()), cx);
                cx.notify();
            });
        })
        .detach();
    }

    // ---- status bar -----------------------------------------------------------

    pub(super) fn render_progress(&self, cx: &App) -> Option<AnyElement> {
        let t = tokens(cx);
        let job = self.ops.job.as_ref()?;
        let done = job.done.load(Ordering::Relaxed);
        let fraction = if job.total == 0 {
            0.0
        } else {
            done as f32 / job.total as f32
        };
        Some(
            h_flex()
                .flex_none()
                .gap(SPACE_4)
                .child(div().text_color(t.fg).child(format!(
                    "{}\u{2026} {}%",
                    job.label,
                    (fraction * 100.0).round() as u32
                )))
                .child(
                    div()
                        .w(px(90.))
                        .child(meter(fraction, t.accent, t.bar_track, px(4.))),
                )
                .into_any_element(),
        )
    }

    // ---- actions --------------------------------------------------------------

    pub(super) fn on_trash(
        &mut self,
        _: &MoveToTrash,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.trash_selected(window, cx);
    }
    pub(super) fn on_new_folder(
        &mut self,
        _: &NewFolder,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.tab().page != Page::Files {
            return cx.propagate();
        }
        self.new_folder(window, cx);
    }
    pub(super) fn on_rename(&mut self, _: &Rename, window: &mut Window, cx: &mut Context<Self>) {
        // With a filter typed, Return opens the match instead (see Open).
        if !self.ops_allowed(window)
            || !self.tab().query.is_empty()
            || self.tab().selection.count() != 1
        {
            return cx.propagate();
        }
        self.begin_rename(window, cx);
    }
    pub(super) fn on_duplicate(
        &mut self,
        _: &Duplicate,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.duplicate(window, cx);
    }
    pub(super) fn on_copy_items(
        &mut self,
        _: &CopyItems,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.copy_selection(false, window, cx);
    }
    pub(super) fn on_cut_items(
        &mut self,
        _: &CutItems,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.copy_selection(true, window, cx);
    }
    pub(super) fn on_paste_items(
        &mut self,
        _: &PasteItems,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.paste(false, window, cx);
    }
    pub(super) fn on_move_items_here(
        &mut self,
        _: &MoveItemsHere,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.paste(true, window, cx);
    }
    pub(super) fn on_undo(&mut self, _: &Undo, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.undo(window, cx);
    }
    pub(super) fn on_redo(&mut self, _: &Redo, window: &mut Window, cx: &mut Context<Self>) {
        if !self.ops_allowed(window) {
            return cx.propagate();
        }
        self.redo(window, cx);
    }
}
