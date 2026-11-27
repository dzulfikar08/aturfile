//! Drag and drop. Items dragged from any view drop onto a folder row, a
//! sidebar place, a side column or the view's background; files dragged in
//! from Finder land the same way, and items dragged out of the window go to
//! Finder (or any app that takes files). Like Finder, a drop on the same disk
//! moves and a drop on another disk copies; Option copies, Command moves.

use std::os::unix::fs::MetadataExt as _;
use std::path::{Path, PathBuf};

use af_fs::Entry;
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::assets::IconName;
use gpui_kit::component::{Icon, h_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Workspace;
use super::list::file_icon;

/// Items being dragged inside the window.
#[derive(Clone)]
pub(crate) struct DraggedFiles {
    /// Each path with whether it is a folder.
    entries: Vec<(PathBuf, bool)>,
    icon: IconName,
    color: Hsla,
    label: SharedString,
}

impl DraggedFiles {
    fn paths(&self) -> Vec<PathBuf> {
        self.entries.iter().map(|(p, _)| p.clone()).collect()
    }
}

/// What follows the pointer during a drag.
struct DragGhost(DraggedFiles);

impl Render for DragGhost {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let n = self.0.entries.len();
        h_flex()
            .gap(SPACE_3)
            .px(SPACE_4)
            .py(SPACE_2)
            .rounded(RADIUS_MD)
            .bg(t.elevated)
            .border_1()
            .border_color(t.border_strong)
            .shadow_md()
            .text_size(FONT_SMALL)
            .text_color(t.fg)
            .opacity(0.92)
            .child(
                Icon::new(self.0.icon)
                    .size(ICON_MD)
                    .text_color(self.0.color),
            )
            .child(div().max_w(px(220.)).truncate().child(self.0.label.clone()))
            .when(n > 1, |d| {
                d.child(
                    div()
                        .px(SPACE_2)
                        .rounded_full()
                        .bg(t.accent)
                        .text_color(t.accent_fg)
                        .text_size(FONT_CAPTION)
                        .child(n.to_string()),
                )
            })
    }
}

/// Whether `dir` takes a drop of `paths`: not into one of them (or a folder
/// inside one), and not where they all already are.
fn accepts(dir: &Path, paths: &[PathBuf]) -> bool {
    !paths.is_empty()
        && !paths.iter().any(|p| dir.starts_with(p))
        && !paths.iter().all(|p| p.parent() == Some(dir))
}

fn device(path: &Path) -> Option<u64> {
    std::fs::metadata(path).ok().map(|m| m.dev())
}

/// Finder's rule: Option copies, Command moves, otherwise move on the same
/// disk and copy across disks.
fn should_move(dir: &Path, paths: &[PathBuf], modifiers: Modifiers) -> bool {
    if modifiers.alt {
        return false;
    }
    if modifiers.platform {
        return true;
    }
    let Some(dest) = device(dir) else {
        return false;
    };
    paths.iter().all(|p| device(p) == Some(dest))
}

impl Workspace {
    /// What a drag starting on `row` carries: the selection when the row is
    /// in it, else the row alone.
    pub(crate) fn drag_payload(&self, row: usize, cx: &App) -> Option<DraggedFiles> {
        let tab = self.tab();
        let entries: Vec<Entry> = if tab.selection.contains(row) {
            tab.selected().into_iter().cloned().collect()
        } else {
            vec![tab.entry_at(row)?.clone()]
        };
        payload(&entries, cx)
    }

    /// Copy or move dropped `paths` into `dir`.
    fn drop_into(
        &mut self,
        dir: PathBuf,
        paths: Vec<PathBuf>,
        window: &Window,
        cx: &mut Context<Self>,
    ) {
        if !accepts(&dir, &paths) {
            return;
        }
        let moving = should_move(&dir, &paths, window.modifiers());
        self.put_into(&dir, &paths, moving, cx);
    }
}

/// A drag payload for `entries`.
pub(crate) fn payload(entries: &[Entry], cx: &App) -> Option<DraggedFiles> {
    let first = entries.first()?;
    let t = tokens(cx);
    let (icon, color) = if entries.len() == 1 {
        file_icon(first, &t)
    } else {
        (IconName::Files, t.fg_muted)
    };
    let label = if entries.len() == 1 {
        first.name.clone().into()
    } else {
        crate::format::items(entries.len()).into()
    };
    Some(DraggedFiles {
        entries: entries.iter().map(|e| (e.path.clone(), e.is_dir)).collect(),
        icon,
        color,
        label,
    })
}

/// Make `el` draggable, carrying `files` (and handing them to the system as
/// real files once the drag leaves the window).
pub(crate) fn drag_source(el: Stateful<Div>, files: Option<DraggedFiles>) -> Stateful<Div> {
    let Some(files) = files else {
        return el;
    };
    el.on_drag(files, |files, _, _, cx| {
        cx.new(|_| DragGhost(files.clone()))
    })
    .external_drag_payload(|files: &DraggedFiles, _, _| {
        Some(ExternalDragPayload::Files(FileDragPaths::new(
            files.entries.iter().cloned(),
        )))
    })
}

/// Make `el` a drop target that puts items into `dir`, tinted while
/// something it takes is over it.
pub(crate) fn drop_target<E>(el: E, dir: PathBuf, ws: WeakEntity<Workspace>, cx: &App) -> E
where
    E: InteractiveElement + Styled,
{
    let tint = tokens(cx).accent_soft;
    let (can_dir, drop_dir, ext_dir) = (dir.clone(), dir.clone(), dir);
    let ext_ws = ws.clone();
    el.drag_over::<DraggedFiles>(move |s, _, _, _| s.bg(tint))
        .drag_over::<ExternalPaths>(move |s, _, _, _| s.bg(tint))
        .can_drop(move |value, _, _| {
            if let Some(files) = value.downcast_ref::<DraggedFiles>() {
                accepts(&can_dir, &files.paths())
            } else if let Some(ext) = value.downcast_ref::<ExternalPaths>() {
                accepts(&can_dir, ext.paths())
            } else {
                false
            }
        })
        .on_drop(move |files: &DraggedFiles, window, cx| {
            let paths = files.paths();
            let _ = ws.update(cx, |ws, cx| {
                ws.drop_into(drop_dir.clone(), paths, window, cx)
            });
        })
        .on_drop(move |ext: &ExternalPaths, window, cx| {
            let paths = ext.paths().to_vec();
            let _ = ext_ws.update(cx, |ws, cx| {
                ws.drop_into(ext_dir.clone(), paths, window, cx)
            });
        })
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::accepts;

    #[test]
    fn refuses_drops_into_self_or_same_place() {
        let a = PathBuf::from("/x/a");
        let b = PathBuf::from("/x/b");
        assert!(!accepts(Path::new("/x/a"), std::slice::from_ref(&a)));
        assert!(!accepts(Path::new("/x/a/sub"), std::slice::from_ref(&a)));
        assert!(!accepts(Path::new("/x"), &[a.clone(), b.clone()]));
        assert!(accepts(Path::new("/x/b"), std::slice::from_ref(&a)));
        assert!(accepts(Path::new("/y"), &[a, b]));
        assert!(!accepts(Path::new("/y"), &[]));
    }
}
