//! The list view: sortable, resizable columns (Name, Modified, Size, Kind),
//! folders first, kind-coloured icons. Virtualised with `uniform_list`.

use af_fs::{Entry, FileKind, SortKey};
use af_ui::metrics::*;
use af_ui::{Tokens, tokens};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::Input;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{Drag, Workspace, menus};
use crate::appearance;
use crate::format;

/// The icon and colour for an entry's kind.
pub(crate) fn file_icon(entry: &Entry, t: &Tokens) -> (IconName, Hsla) {
    kind_icon(entry.kind(), entry.is_symlink, t)
}

pub(crate) fn kind_icon(kind: FileKind, symlink: bool, t: &Tokens) -> (IconName, Hsla) {
    let (icon, color) = match kind {
        FileKind::Folder => (IconName::Folder, t.kind_folder),
        FileKind::Package => (IconName::Package, t.kind_app),
        FileKind::Application => (IconName::AppWindow, t.kind_app),
        FileKind::Image => (IconName::FileImage, t.kind_image),
        FileKind::Video => (IconName::FileVideoCamera, t.kind_media),
        FileKind::Audio => (IconName::FileMusic, t.kind_audio),
        FileKind::Archive => (IconName::FileArchive, t.kind_archive),
        FileKind::DiskImage => (IconName::HardDrive, t.kind_archive),
        FileKind::Code => (IconName::FileCode, t.kind_code),
        FileKind::Markdown | FileKind::Text => (IconName::FileText, t.kind_document),
        FileKind::Pdf | FileKind::Document => (IconName::FileText, t.kind_media),
        FileKind::Spreadsheet => (IconName::FileSpreadsheet, t.kind_code),
        FileKind::Presentation => (IconName::Presentation, t.kind_archive),
        FileKind::Other => (IconName::File, t.kind_other),
    };
    if symlink && kind != FileKind::Folder {
        (IconName::FileSymlink, color)
    } else {
        (icon, color)
    }
}

/// The least width the Name column keeps before other columns hide.
const NAME_MIN: f32 = 220.;

pub(crate) fn size_text(entry: &Entry) -> String {
    if entry.is_dir {
        "\u{2014}".into()
    } else {
        af_fs::format_size(entry.size)
    }
}

impl Workspace {
    pub(crate) fn set_sort(&mut self, key: SortKey, cx: &mut Context<Self>) {
        let show_hidden = self.show_hidden;
        let tab = self.tab_mut();
        if tab.sort == key {
            tab.ascending = !tab.ascending;
        } else {
            tab.sort = key;
            tab.ascending = matches!(key, SortKey::Name | SortKey::Kind);
        }
        let keep = tab.selected_paths();
        let cursor = tab.current().map(|e| e.path.clone());
        tab.sort_entries();
        tab.refilter(show_hidden);
        tab.reselect(&keep, cursor.as_deref());
        self.scroll_to_cursor();
        cx.notify();
    }

    fn render_header(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let tab = self.tab();
        let (sort, ascending) = (tab.sort, tab.ascending);
        let widths = self.settings.columns;
        let column =
            |id: &'static str, label: &'static str, key: SortKey, cx: &mut Context<Self>| {
                let active = sort == key;
                h_flex()
                    .id(id)
                    .h_full()
                    .gap(SPACE_2)
                    .cursor_pointer()
                    .when(active, |d| d.text_color(t.fg))
                    .hover(|s| s.text_color(t.fg))
                    .child(label)
                    .when(active, |d| {
                        d.child(
                            Icon::new(if ascending {
                                IconName::ChevronUp
                            } else {
                                IconName::ChevronDown
                            })
                            .size(ICON_SM),
                        )
                    })
                    .on_click(cx.listener(move |ws, _: &ClickEvent, _, cx| ws.set_sort(key, cx)))
            };
        let resizable = |ix: usize, el: Stateful<Div>, cx: &mut Context<Self>| {
            let start_w = widths[ix];
            div()
                .relative()
                .flex_none()
                .h_full()
                .w(px(widths[ix]))
                .pl(SPACE_5)
                .flex()
                .items_center()
                .child(el)
                .child(
                    div()
                        .id(("col-edge", ix))
                        .absolute()
                        .left(px(-3.))
                        .top(px(5.))
                        .bottom(px(5.))
                        .w(px(6.))
                        .cursor(CursorStyle::ResizeLeftRight)
                        .child(div().mx_auto().w(px(1.)).h_full().bg(t.border))
                        .on_mouse_down(
                            MouseButton::Left,
                            cx.listener(move |ws, e: &MouseDownEvent, _, cx| {
                                ws.drag = Some(Drag::Column {
                                    ix,
                                    start_x: e.position.x,
                                    start_w,
                                });
                                cx.stop_propagation();
                            }),
                        ),
                )
        };
        let shown = self.list_cols;
        let name = column("h-name", "Name", SortKey::Name, cx);
        let modified = column("h-date", "Modified", SortKey::Modified, cx);
        let size = column("h-size", "Size", SortKey::Size, cx);
        let kind = column("h-kind", "Kind", SortKey::Kind, cx);
        h_flex()
            .flex_none()
            .w_full()
            .h(HEADER_HEIGHT)
            .px(SPACE_5)
            .text_size(FONT_SMALL)
            .font_weight(FontWeight::MEDIUM)
            .text_color(t.fg_muted)
            .border_b_1()
            .border_color(t.border)
            .child(div().flex_1().min_w_0().h_full().pl(px(26.)).child(name))
            .when(shown[0], |d| d.child(resizable(0, modified, cx)))
            .when(shown[1], |d| d.child(resizable(1, size, cx)))
            .when(shown[2], |d| d.child(resizable(2, kind, cx)))
    }

    /// Drop Kind, then Modified, then Size while the names would get less
    /// than a readable width.
    fn fit_list_columns(&mut self, window: &Window) {
        let mut avail = f32::from(window.viewport_size().width) - 40.0;
        if self.settings.show_sidebar {
            avail -= self.settings.sidebar_width;
        }
        if self.settings.show_preview {
            avail -= self.settings.preview_width;
        }
        let w = self.settings.columns;
        let mut shown = [true; 3];
        for drop in [2, 0, 1] {
            let used: f32 = (0..3).filter(|&i| shown[i]).map(|i| w[i]).sum();
            if avail - used >= NAME_MIN {
                break;
            }
            shown[drop] = false;
        }
        self.list_cols = shown;
    }

    pub(super) fn render_list(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        self.fit_list_columns(window);
        let t = tokens(cx);
        let ws = cx.entity().downgrade();
        let tab = self.tab();
        let empty = tab.visible.is_empty();
        let body = if empty {
            let msg = if !tab.loaded {
                ""
            } else if tab.query.is_empty() {
                "This folder is empty"
            } else {
                "No names match"
            };
            div()
                .id("list-empty")
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.fg_dim)
                .child(msg)
                .into_any_element()
        } else {
            uniform_list(
                "files",
                tab.visible.len(),
                cx.processor(|ws, range: std::ops::Range<usize>, _, cx| {
                    range
                        .filter_map(|row| ws.list_row(row, cx))
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&tab.scroll)
            .flex_1()
            .py(SPACE_2)
            .into_any_element()
        };
        v_flex()
            .id("list-view")
            .size_full()
            .child(self.render_header(cx))
            .child(
                v_flex()
                    .id("list-body")
                    .flex_1()
                    .min_h_0()
                    .on_mouse_down(
                        MouseButton::Right,
                        cx.listener(|ws, _: &MouseDownEvent, _, cx| ws.right_click_background(cx)),
                    )
                    .on_click(cx.listener(|ws, e: &ClickEvent, window, cx| {
                        // A click below the last row clears the selection.
                        if e.standard_click() {
                            ws.tab_mut().selection.clear();
                            ws.focus_list(window, cx);
                            cx.notify();
                        }
                    }))
                    .child(body)
                    .context_menu(move |menu, _, cx| menus::background_menu(menu, &ws, cx)),
            )
    }

    fn list_row(&mut self, row: usize, cx: &mut Context<Self>) -> Option<AnyElement> {
        let t = tokens(cx);
        let row_h = appearance::density(cx).row_height();
        let widths = self.settings.columns;
        let shown = self.list_cols;
        let tab = self.tab();
        let entry = tab.entry_at(row)?.clone();
        let selected = tab.selection.contains(row);
        let (icon, color) = file_icon(&entry, &t);
        let date = entry
            .modified
            .map(format::relative_time)
            .unwrap_or_default();
        let kind = af_fs::kind::kind_label(&entry.name, entry.is_dir);
        let renaming = self
            .ops
            .renaming
            .as_ref()
            .is_some_and(|r| r.path == entry.path);
        let ws = cx.entity().downgrade();

        let name: AnyElement = if renaming {
            div()
                .flex_1()
                .min_w_0()
                .child(Input::new(&self.ops.rename_input).xsmall())
                .into_any_element()
        } else {
            div()
                .flex_1()
                .min_w_0()
                .truncate()
                .child(entry.name.clone())
                .into_any_element()
        };
        let muted = |w: f32, text: String| {
            div()
                .flex_none()
                .w(px(w))
                .pl(SPACE_5)
                .truncate()
                .text_color(t.fg_muted)
                .child(text)
        };
        Some(
            h_flex()
                .id(("row", row))
                .w_full()
                .h(row_h)
                .px(SPACE_3)
                .child(
                    h_flex()
                        .size_full()
                        .px(SPACE_3)
                        .rounded(RADIUS_MD)
                        .when_else(
                            selected,
                            |d| d.bg(t.selected),
                            |d| d.hover(|s| s.bg(t.hover)),
                        )
                        .when(entry.hidden, |d| d.opacity(0.6))
                        .child(
                            h_flex()
                                .flex_1()
                                .min_w_0()
                                .gap(SPACE_4)
                                .child(Icon::new(icon).size(ICON_LG).text_color(color).flex_none())
                                .child(name),
                        )
                        .when(shown[0], |d| d.child(muted(widths[0], date)))
                        .when(shown[1], |d| {
                            d.child(muted(widths[1], size_text(&entry)).flex().justify_end())
                        })
                        .when(shown[2], |d| d.child(muted(widths[2], kind))),
                )
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                        if e.click_count == 1 {
                            ws.click_row(row, e.modifiers, window, cx);
                        }
                    }),
                )
                .on_mouse_down(
                    MouseButton::Right,
                    cx.listener(move |ws, _: &MouseDownEvent, _, cx| ws.right_click_row(row, cx)),
                )
                .on_click(cx.listener(move |ws, e: &ClickEvent, _, cx| {
                    cx.stop_propagation();
                    if e.click_count() >= 2 {
                        ws.open_row(row, cx);
                    }
                }))
                .context_menu(move |menu, _, cx| menus::item_menu(menu, &ws, cx))
                .into_any_element(),
        )
    }
}
