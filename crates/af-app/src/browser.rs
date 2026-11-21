//! The window: sidebar, file list with type-to-filter, and the cleanup pane.

use std::path::{Path, PathBuf};

use af_fs::{Entry, SortKey};
use gpui::{
    App, ClickEvent, Context, FocusHandle, Focusable, KeyDownEvent, ScrollStrategy, SharedString,
    Task, UniformListScrollHandle, Window, div, prelude::*, px, uniform_list,
};

use crate::cleanup::Cleanup;
use crate::theme;
use crate::*;

const ROW_HEIGHT: f32 = 26.0;
const PAGE: usize = 20;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Files,
    Cleanup,
}

pub struct Workspace {
    focus: FocusHandle,
    pub mode: Mode,
    cwd: PathBuf,
    /// Every entry of `cwd`, sorted; hidden ones included.
    entries: Vec<Entry>,
    /// Indexes into `entries` after the hidden and text filters.
    visible: Vec<usize>,
    selected: usize,
    query: String,
    show_hidden: bool,
    sort: SortKey,
    ascending: bool,
    back: Vec<PathBuf>,
    forward: Vec<PathBuf>,
    status: Option<SharedString>,
    error: Option<SharedString>,
    load_ms: u128,
    scroll: UniformListScrollHandle,
    _load: Option<Task<()>>,
    pub cleanup: Cleanup,
    /// Folders the App Sandbox lets us read (unused outside the sandbox).
    grants: af_mac::Grants,
}

impl Workspace {
    pub fn new(start: PathBuf, mode: Mode, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus = cx.focus_handle();
        window.focus(&focus);
        let mut this = Self {
            focus,
            mode: Mode::Files,
            cwd: start.clone(),
            entries: Vec::new(),
            visible: Vec::new(),
            selected: 0,
            query: String::new(),
            show_hidden: false,
            sort: SortKey::Name,
            ascending: true,
            back: Vec::new(),
            forward: Vec::new(),
            status: None,
            error: None,
            load_ms: 0,
            scroll: UniformListScrollHandle::new(),
            _load: None,
            cleanup: Cleanup::default(),
            grants: af_mac::Grants::restore(),
        };
        this.load(start, None, cx);
        if mode == Mode::Cleanup {
            this.show_cleanup(cx);
        }
        this
    }

    // ---- navigation -------------------------------------------------------

    /// Read `dir` off the UI thread; select `select` (a child name) when done.
    fn load(&mut self, dir: PathBuf, select: Option<String>, cx: &mut Context<Self>) {
        let read_dir = dir.clone();
        let task = cx.background_executor().spawn(async move {
            let started = std::time::Instant::now();
            let listed = af_fs::list_dir(&read_dir);
            (listed, started.elapsed().as_millis())
        });
        self._load = Some(cx.spawn(async move |this, cx| {
            let (listed, ms) = task.await;
            let _ = this.update(cx, |ws, cx| {
                match listed {
                    Ok(mut entries) => {
                        af_fs::sort(&mut entries, ws.sort, ws.ascending);
                        ws.cwd = dir;
                        ws.entries = entries;
                        ws.error = None;
                    }
                    Err(e) => {
                        ws.error = Some(format!("Can't open {}: {e}", dir.display()).into());
                    }
                }
                ws.load_ms = ms;
                ws.query.clear();
                ws.refilter();
                ws.selected = select
                    .and_then(|name| ws.visible.iter().position(|&i| ws.entries[i].name == name))
                    .unwrap_or(0);
                ws.scroll
                    .scroll_to_item(ws.selected, ScrollStrategy::Center);
                cx.notify();
            });
        }));
    }

    fn go(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        if dir == self.cwd {
            return;
        }
        self.back.push(self.cwd.clone());
        self.forward.clear();
        self.load(dir, None, cx);
    }

    fn refilter(&mut self) {
        let shown: Vec<usize> = (0..self.entries.len())
            .filter(|&i| self.show_hidden || !self.entries[i].hidden)
            .collect();
        let subset: Vec<Entry> = shown.iter().map(|&i| self.entries[i].clone()).collect();
        self.visible = af_fs::filter(&subset, &self.query)
            .into_iter()
            .map(|j| shown[j])
            .collect();
        self.selected = self.selected.min(self.visible.len().saturating_sub(1));
    }

    fn current(&self) -> Option<&Entry> {
        self.visible.get(self.selected).map(|&i| &self.entries[i])
    }

    fn select(&mut self, ix: usize, cx: &mut Context<Self>) {
        if self.visible.is_empty() {
            return;
        }
        self.selected = ix.min(self.visible.len() - 1);
        self.scroll
            .scroll_to_item(self.selected, ScrollStrategy::Top);
        cx.notify();
    }

    fn open_entry(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.current().cloned() else {
            return;
        };
        if entry.is_dir && !is_package(&entry.path) {
            self.go(entry.path, cx);
        } else {
            cx.open_with_system(&entry.path);
        }
    }

    // ---- actions ----------------------------------------------------------

    fn on_next(&mut self, _: &SelectNext, _: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Files => self.select(self.selected + 1, cx),
            Mode::Cleanup => self.cleanup.move_by(1, cx),
        }
    }
    fn on_prev(&mut self, _: &SelectPrev, _: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Files => self.select(self.selected.saturating_sub(1), cx),
            Mode::Cleanup => self.cleanup.move_by(-1, cx),
        }
    }
    fn on_first(&mut self, _: &SelectFirst, _: &mut Window, cx: &mut Context<Self>) {
        self.select(0, cx);
    }
    fn on_last(&mut self, _: &SelectLast, _: &mut Window, cx: &mut Context<Self>) {
        self.select(usize::MAX, cx);
    }
    fn on_page_down(&mut self, _: &PageDown, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected + PAGE, cx);
    }
    fn on_page_up(&mut self, _: &PageUp, _: &mut Window, cx: &mut Context<Self>) {
        self.select(self.selected.saturating_sub(PAGE), cx);
    }
    fn on_open(&mut self, _: &Open, _: &mut Window, cx: &mut Context<Self>) {
        match self.mode {
            Mode::Files => self.open_entry(cx),
            Mode::Cleanup => self.cleanup.toggle_selected(cx),
        }
    }
    fn on_parent(&mut self, _: &Parent, _: &mut Window, cx: &mut Context<Self>) {
        self.up(cx);
    }
    fn up(&mut self, cx: &mut Context<Self>) {
        let Some(parent) = self.cwd.parent().map(Path::to_path_buf) else {
            return;
        };
        let child = self
            .cwd
            .file_name()
            .map(|n| n.to_string_lossy().into_owned());
        self.back.push(self.cwd.clone());
        self.forward.clear();
        self.load(parent, child, cx);
    }
    fn on_back(&mut self, _: &Back, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(dir) = self.back.pop() {
            self.forward.push(self.cwd.clone());
            self.load(dir, None, cx);
        }
    }
    fn on_forward(&mut self, _: &Forward, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(dir) = self.forward.pop() {
            self.back.push(self.cwd.clone());
            self.load(dir, None, cx);
        }
    }
    fn on_toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        self.refilter();
        cx.notify();
    }
    fn on_reveal(&mut self, _: &Reveal, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(entry) = self.current() {
            cx.reveal_path(&entry.path.clone());
        }
    }
    fn on_trash(&mut self, _: &MoveToTrash, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Files {
            return;
        }
        let Some(entry) = self.current().cloned() else {
            return;
        };
        // The same guard rails as cleanup: protected paths are refused.
        let item = af_clean::cleanup::StagedItem {
            path: entry.path.clone(),
            logical: entry.size,
            category: "manual".into(),
            reason: "moved from the file list".into(),
        };
        let outcome = af_clean::cleanup::apply(&[item], af_clean::cleanup::Disposal::Trash, false);
        let msg = match outcome.first() {
            Some(o) if o.ok => format!("Moved \u{201c}{}\u{201d} to Trash", entry.name),
            Some(o) => format!("Not moved: {}", o.message),
            None => String::new(),
        };
        self.status = Some(msg.into());
        let keep = self.selected;
        self.load(self.cwd.clone(), None, cx);
        self.selected = keep;
    }
    fn on_show_browser(&mut self, _: &ShowBrowser, _: &mut Window, cx: &mut Context<Self>) {
        self.mode = Mode::Files;
        cx.notify();
    }
    fn on_show_cleanup(&mut self, _: &ShowCleanup, _: &mut Window, cx: &mut Context<Self>) {
        self.show_cleanup(cx);
    }
    fn show_cleanup(&mut self, cx: &mut Context<Self>) {
        self.mode = Mode::Cleanup;
        if !self.cleanup.started() && self.grants.covers(&af_clean::paths::home()) {
            Cleanup::start_scan(self, cx);
        }
        cx.notify();
    }
    fn on_rescan(&mut self, _: &Rescan, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Cleanup {
            Cleanup::start_scan(self, cx);
        } else {
            self.load(self.cwd.clone(), None, cx);
        }
    }
    fn on_stage(&mut self, _: &StageSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Cleanup {
            self.cleanup.toggle_selected(cx);
        }
    }
    fn on_empty_queue(&mut self, _: &EmptyQueue, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode == Mode::Cleanup {
            Cleanup::trash_queue(self, cx);
        }
    }

    /// Type-to-filter: printable keys build the query, Backspace edits it (or
    /// goes up a folder when it's empty), Escape clears it, Space previews.
    fn on_key(&mut self, event: &KeyDownEvent, _: &mut Window, cx: &mut Context<Self>) {
        if self.mode != Mode::Files {
            return;
        }
        let ks = &event.keystroke;
        if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
            return;
        }
        match ks.key.as_str() {
            "backspace" => {
                if self.query.pop().is_none() {
                    self.up(cx);
                    return;
                }
            }
            "escape" => {
                if self.query.is_empty() {
                    return;
                }
                self.query.clear();
            }
            "space" if self.query.is_empty() => {
                if let Some(entry) = self.current() {
                    quick_look(&entry.path.clone(), cx);
                }
                cx.stop_propagation();
                return;
            }
            _ => match ks.key_char.as_deref() {
                Some(c) if !c.is_empty() && !c.chars().any(char::is_control) => {
                    self.query.push_str(c)
                }
                _ => return,
            },
        }
        self.selected = 0;
        self.refilter();
        self.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.stop_propagation();
        cx.notify();
    }

    fn set_sort(&mut self, key: SortKey, cx: &mut Context<Self>) {
        if self.sort == key {
            self.ascending = !self.ascending;
        } else {
            self.sort = key;
            self.ascending = key == SortKey::Name || key == SortKey::Kind;
        }
        let keep = self.current().map(|e| e.path.clone());
        af_fs::sort(&mut self.entries, self.sort, self.ascending);
        self.refilter();
        if let Some(path) = keep {
            if let Some(pos) = self
                .visible
                .iter()
                .position(|&i| self.entries[i].path == path)
            {
                self.select(pos, cx);
            }
        }
        cx.notify();
    }

    // ---- rendering --------------------------------------------------------

    fn render_sidebar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let home = af_clean::paths::home();
        let places: Vec<(&str, PathBuf)> = vec![
            ("Home", home.clone()),
            ("Desktop", home.join("Desktop")),
            ("Documents", home.join("Documents")),
            ("Downloads", home.join("Downloads")),
            ("Applications", PathBuf::from("/Applications")),
            ("Computer", PathBuf::from("/")),
        ];
        let mut col = div()
            .w(px(190.0))
            .h_full()
            .flex()
            .flex_col()
            .bg(theme::sidebar())
            .border_r_1()
            .border_color(theme::border())
            .pt(px(40.0))
            .px_2()
            .gap(px(2.0))
            .child(section_label("Places"));
        for (i, (label, path)) in places.into_iter().enumerate() {
            if !path.is_dir() {
                continue;
            }
            let active = self.mode == Mode::Files && self.cwd == path;
            let target = path.clone();
            col = col.child(
                sidebar_item(("place", i), label, active).on_click(cx.listener(
                    move |ws, _: &ClickEvent, _, cx| {
                        ws.mode = Mode::Files;
                        ws.go(target.clone(), cx);
                    },
                )),
            );
        }
        col.child(div().h(px(14.0)))
            .child(section_label("Tools"))
            .child(
                sidebar_item("cleanup", "Clean Up", self.mode == Mode::Cleanup)
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.show_cleanup(cx))),
            )
    }

    fn render_toolbar(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let crumbs = breadcrumbs(&self.cwd);
        let mut bar = div()
            .h(px(40.0))
            .flex()
            .items_center()
            .gap_1()
            .px_3()
            .border_b_1()
            .border_color(theme::border())
            .child(
                tool_button("back", "\u{2039}", !self.back.is_empty())
                    .on_click(cx.listener(|ws, _: &ClickEvent, w, cx| ws.on_back(&Back, w, cx))),
            )
            .child(
                tool_button("fwd", "\u{203a}", !self.forward.is_empty()).on_click(
                    cx.listener(|ws, _: &ClickEvent, w, cx| ws.on_forward(&Forward, w, cx)),
                ),
            )
            .child(div().w(px(8.0)));
        let last = crumbs.len().saturating_sub(1);
        for (i, (label, path)) in crumbs.into_iter().enumerate() {
            if i > 0 {
                bar = bar.child(div().text_color(theme::muted()).child("/"));
            }
            bar = bar.child(
                div()
                    .id(("crumb", i))
                    .px_1()
                    .rounded_sm()
                    .cursor_pointer()
                    .text_color(if i == last {
                        theme::text()
                    } else {
                        theme::muted()
                    })
                    .hover(|s| s.bg(theme::hover()))
                    .child(label)
                    .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| {
                        ws.go(path.clone(), cx);
                    })),
            );
        }
        bar.child(div().flex_1()).child(
            div()
                .min_w(px(180.0))
                .h(px(26.0))
                .px_2()
                .flex()
                .items_center()
                .rounded_md()
                .bg(theme::panel())
                .border_1()
                .border_color(if self.query.is_empty() {
                    theme::border()
                } else {
                    theme::accent()
                })
                .text_color(if self.query.is_empty() {
                    theme::muted()
                } else {
                    theme::text()
                })
                .child(if self.query.is_empty() {
                    SharedString::from("Type to filter")
                } else {
                    SharedString::from(format!("{}\u{2502}", self.query))
                }),
        )
    }

    fn render_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let arrow = |key: SortKey| {
            if self.sort == key {
                if self.ascending {
                    " \u{25b4}"
                } else {
                    " \u{25be}"
                }
            } else {
                ""
            }
        };
        let col = |id: &'static str, label: &str, key: SortKey, cx: &mut Context<Self>| {
            div()
                .id(id)
                .cursor_pointer()
                .hover(|s| s.text_color(theme::text()))
                .child(format!("{label}{}", arrow(key)))
                .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.set_sort(key, cx)))
        };
        div()
            .h(px(24.0))
            .flex()
            .items_center()
            .px_3()
            .text_xs()
            .text_color(theme::muted())
            .border_b_1()
            .border_color(theme::border())
            .child(col("h-name", "Name", SortKey::Name, cx).flex_1())
            .child(col("h-date", "Modified", SortKey::Modified, cx).w(px(150.0)))
            .child(
                col("h-size", "Size", SortKey::Size, cx)
                    .w(px(90.0))
                    .flex()
                    .justify_end(),
            )
            .child(col("h-kind", "Kind", SortKey::Kind, cx).w(px(90.0)).pl_4())
    }

    /// App Store builds: ask for a folder in the open panel, then keep it.
    fn request_access(&mut self, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(gpui::PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Allow Access".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = answer.await else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                for path in &paths {
                    if let Err(e) = ws.grants.add(path) {
                        ws.status = Some(format!("Couldn't keep access: {e}").into());
                    }
                }
                ws.load(ws.cwd.clone(), None, cx);
                if ws.mode == Mode::Cleanup {
                    ws.show_cleanup(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }

    pub fn render_grant(&self, folder: &Path, cx: &mut Context<Self>) -> gpui::AnyElement {
        let home = af_clean::paths::home();
        let name = af_clean::paths::display_path(folder, &home);
        div()
            .flex_1()
            .flex()
            .flex_col()
            .items_center()
            .justify_center()
            .gap_3()
            .px_8()
            .child(
                div()
                    .text_lg()
                    .child(format!("Allow aturfile to open {name}")),
            )
            .child(div().max_w(px(460.0)).text_color(theme::muted()).child(
                "macOS asks you to choose each folder an App Store app may open. \
                         Choose your Home folder once to browse and clean everything in it.",
            ))
            .child(
                tool_button("grant", "Choose Folder\u{2026}", true)
                    .h(px(32.0))
                    .px_4()
                    .bg(theme::accent())
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.request_access(cx))),
            )
            .into_any_element()
    }

    fn render_list(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if !self.grants.covers(&self.cwd) {
            return self.render_grant(&self.cwd.clone(), cx);
        }
        if let Some(error) = &self.error {
            return div()
                .flex_1()
                .p_6()
                .text_color(theme::danger())
                .child(error.clone())
                .into_any_element();
        }
        if self.visible.is_empty() {
            let msg = if self.query.is_empty() {
                "This folder is empty"
            } else {
                "No names match"
            };
            return div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(theme::muted())
                .child(msg)
                .into_any_element();
        }
        uniform_list(
            "files",
            self.visible.len(),
            cx.processor(|ws, range: std::ops::Range<usize>, _, cx| {
                range
                    .map(|ix| {
                        let entry = &ws.entries[ws.visible[ix]];
                        file_row(ix, entry, ix == ws.selected, cx)
                    })
                    .collect::<Vec<_>>()
            }),
        )
        .track_scroll(self.scroll.clone())
        .flex_1()
        .into_any_element()
    }

    fn render_status(&self) -> impl IntoElement {
        let shown = self.visible.len();
        let total = self.entries.len();
        let mut text = format!("{shown} items");
        if shown != total {
            text.push_str(&format!(" of {total}"));
        }
        text.push_str(&format!("  \u{00b7}  read in {} ms", self.load_ms));
        if self.show_hidden {
            text.push_str("  \u{00b7}  showing hidden");
        }
        div()
            .h(px(24.0))
            .flex()
            .items_center()
            .px_3()
            .gap_4()
            .text_xs()
            .text_color(theme::muted())
            .border_t_1()
            .border_color(theme::border())
            .child(text)
            .child(div().flex_1())
            .children(self.status.clone())
    }
}

fn file_row(
    ix: usize,
    entry: &Entry,
    selected: bool,
    cx: &mut Context<Workspace>,
) -> gpui::Stateful<gpui::Div> {
    let date = entry.modified.map(format_time).unwrap_or_default();
    let size = if entry.is_dir {
        "\u{2014}".to_string()
    } else {
        af_fs::format_size(entry.size)
    };
    let kind = if entry.is_dir {
        if is_package(&entry.path) {
            "Package".into()
        } else {
            "Folder".into()
        }
    } else {
        let ext = entry.extension();
        if ext.is_empty() {
            "Document".to_string()
        } else {
            ext.to_uppercase()
        }
    };
    let icon = if entry.is_dir { "\u{25a0}" } else { "\u{25a1}" };
    div()
        .id(("row", ix))
        .w_full()
        .h(px(ROW_HEIGHT))
        .flex()
        .items_center()
        .px_3()
        .mx_1()
        .rounded_md()
        .cursor_default()
        .when(selected, |s| s.bg(theme::selected()))
        .when(!selected, |s| s.hover(|s| s.bg(theme::hover())))
        .when(entry.hidden, |s| s.opacity(0.55))
        .child(
            div()
                .flex_1()
                .min_w_0()
                .flex()
                .items_center()
                .gap_2()
                .child(
                    div()
                        .text_xs()
                        .text_color(if entry.is_dir {
                            theme::folder()
                        } else {
                            theme::muted()
                        })
                        .child(icon),
                )
                .child(div().truncate().child(entry.name.clone()))
                .when(entry.is_symlink, |s| {
                    s.child(div().text_xs().text_color(theme::muted()).child("\u{2197}"))
                }),
        )
        .child(
            div()
                .w(px(150.0))
                .text_sm()
                .text_color(theme::muted())
                .child(date),
        )
        .child(
            div()
                .w(px(90.0))
                .flex()
                .justify_end()
                .text_sm()
                .text_color(theme::muted())
                .child(size),
        )
        .child(
            div()
                .w(px(90.0))
                .pl_4()
                .text_sm()
                .text_color(theme::muted())
                .truncate()
                .child(kind),
        )
        .on_click(cx.listener(move |ws, event: &ClickEvent, _, cx| {
            ws.select(ix, cx);
            if event.click_count() >= 2 {
                ws.open_entry(cx);
            }
        }))
}

/// macOS bundles (.app, .pages, …) open as documents, like in Finder.
fn is_package(path: &Path) -> bool {
    const PACKAGES: &[&str] = &[
        "app",
        "bundle",
        "framework",
        "pages",
        "numbers",
        "key",
        "photoslibrary",
        "xcodeproj",
        "xcworkspace",
        "playground",
        "rtfd",
        "pkg",
    ];
    cfg!(target_os = "macos")
        && path
            .extension()
            .map(|e| PACKAGES.contains(&e.to_string_lossy().to_lowercase().as_str()))
            .unwrap_or(false)
}

fn quick_look(path: &Path, cx: &mut App) {
    if cfg!(target_os = "macos") {
        // TODO(pro): an in-window QLPreviewPanel instead of qlmanage.
        let _ = std::process::Command::new("qlmanage")
            .arg("-p")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    } else {
        cx.open_with_system(path);
    }
}

fn breadcrumbs(path: &Path) -> Vec<(String, PathBuf)> {
    let home = af_clean::paths::home();
    let mut out = Vec::new();
    let mut acc = PathBuf::new();
    for comp in path.components() {
        acc.push(comp);
        let label = if acc == home {
            out.clear();
            "~".to_string()
        } else {
            match comp {
                std::path::Component::RootDir => "/".to_string(),
                other => other.as_os_str().to_string_lossy().into_owned(),
            }
        };
        out.push((label, acc.clone()));
    }
    // Long paths: keep the first crumb and the last four.
    if out.len() > 6 {
        let tail = out.split_off(out.len() - 4);
        out.truncate(1);
        out.push((
            "\u{2026}".into(),
            tail[0]
                .1
                .parent()
                .map(Path::to_path_buf)
                .unwrap_or_default(),
        ));
        out.extend(tail);
    }
    out
}

fn format_time(secs: i64) -> String {
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let age = now - secs;
    if age < 0 {
        return "In the future".into();
    }
    match age {
        0..60 => "Just now".into(),
        60..3600 => format!("{} min ago", age / 60),
        3600..86_400 => format!("{} h ago", age / 3600),
        86_400..172_800 => "Yesterday".into(),
        _ => civil_date(secs),
    }
}

/// YYYY-MM-DD in UTC without pulling a date crate into the UI.
fn civil_date(secs: i64) -> String {
    let days = secs.div_euclid(86_400);
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + if m <= 2 { 1 } else { 0 };
    format!("{y:04}-{m:02}-{d:02}")
}

pub fn section_label(text: &'static str) -> impl IntoElement {
    div()
        .px_2()
        .pt_1()
        .pb_1()
        .text_xs()
        .text_color(theme::muted())
        .child(text)
}

fn sidebar_item(
    id: impl Into<gpui::ElementId>,
    label: &str,
    active: bool,
) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .h(px(26.0))
        .px_2()
        .flex()
        .items_center()
        .rounded_md()
        .cursor_pointer()
        .text_sm()
        .when(active, |s| s.bg(theme::selected()))
        .when(!active, |s| s.hover(|s| s.bg(theme::hover())))
        .child(label.to_string())
}

pub fn tool_button(id: &'static str, label: &str, enabled: bool) -> gpui::Stateful<gpui::Div> {
    div()
        .id(id)
        .min_w(px(26.0))
        .h(px(26.0))
        .px_2()
        .flex()
        .items_center()
        .justify_center()
        .rounded_md()
        .text_color(if enabled {
            theme::text()
        } else {
            theme::border()
        })
        .when(enabled, |s| {
            s.cursor_pointer().hover(|s| s.bg(theme::hover()))
        })
        .child(label.to_string())
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let main = match self.mode {
            Mode::Files => div()
                .flex_1()
                .min_w_0()
                .flex()
                .flex_col()
                .child(self.render_toolbar(cx))
                .child(self.render_header(cx))
                .child(self.render_list(cx))
                .child(self.render_status())
                .into_any_element(),
            Mode::Cleanup if !self.grants.covers(&af_clean::paths::home()) => {
                self.render_grant(&af_clean::paths::home(), cx)
            }
            Mode::Cleanup => self.render_cleanup(cx).into_any_element(),
        };
        div()
            .id("workspace")
            .key_context("Workspace")
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::on_next))
            .on_action(cx.listener(Self::on_prev))
            .on_action(cx.listener(Self::on_first))
            .on_action(cx.listener(Self::on_last))
            .on_action(cx.listener(Self::on_page_down))
            .on_action(cx.listener(Self::on_page_up))
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_parent))
            .on_action(cx.listener(Self::on_back))
            .on_action(cx.listener(Self::on_forward))
            .on_action(cx.listener(Self::on_toggle_hidden))
            .on_action(cx.listener(Self::on_reveal))
            .on_action(cx.listener(Self::on_trash))
            .on_action(cx.listener(Self::on_show_browser))
            .on_action(cx.listener(Self::on_show_cleanup))
            .on_action(cx.listener(Self::on_rescan))
            .on_action(cx.listener(Self::on_stage))
            .on_action(cx.listener(Self::on_empty_queue))
            .size_full()
            .flex()
            .bg(theme::bg())
            .text_color(theme::text())
            .text_sm()
            .child(self.render_sidebar(cx))
            .child(main)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil_date(0), "1970-01-01");
        assert_eq!(civil_date(1_795_000_000), "2026-11-18");
    }
}
