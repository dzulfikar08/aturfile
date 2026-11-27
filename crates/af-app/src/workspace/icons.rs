//! The icon view: a grid of large icons with names; small images show a
//! thumbnail. Virtualised by rows with `uniform_list`.

use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::list::file_icon;
use super::{Workspace, dnd, menus};
use crate::tab::is_folder;

/// Images up to this size get a thumbnail (GPUI decodes the whole file).
const THUMBNAIL_MAX_BYTES: u64 = 12 * 1024 * 1024;

impl Workspace {
    pub(super) fn render_icons(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = tokens(cx);
        let ws = cx.entity().downgrade();
        let mut avail = f32::from(window.viewport_size().width) - 2.0 * f32::from(SPACE_6);
        if self.settings.show_sidebar {
            avail -= self.settings.sidebar_width;
        }
        if self.settings.show_preview {
            avail -= self.settings.preview_width;
        }
        let cols = ((avail / ICON_CELL_WIDTH).floor() as usize).max(1);
        let tab = self.tab_mut();
        tab.icon_cols = cols;
        let count = tab.visible.len();
        let rows = count.div_ceil(cols);
        let loaded = tab.loaded;
        let body = if count == 0 {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_color(t.fg_dim)
                .child(if loaded { "This folder is empty" } else { "" })
                .into_any_element()
        } else {
            uniform_list(
                "icons",
                rows,
                cx.processor(move |ws, range: std::ops::Range<usize>, _, cx| {
                    range.map(|r| ws.icon_row(r, cols, cx)).collect::<Vec<_>>()
                }),
            )
            .track_scroll(&self.tab().scroll)
            .flex_1()
            .py(SPACE_4)
            .into_any_element()
        };
        let cwd = self.tab().cwd.clone();
        dnd::drop_target(v_flex().id("icons-view").size_full(), cwd, ws.clone(), cx)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|ws, _: &MouseDownEvent, _, cx| ws.right_click_background(cx)),
            )
            .on_click(cx.listener(|ws, e: &ClickEvent, window, cx| {
                if e.standard_click() {
                    ws.tab_mut().selection.clear();
                    ws.focus_list(window, cx);
                    cx.notify();
                }
            }))
            .child(body)
            .context_menu(move |menu, _, cx| menus::background_menu(menu, &ws, cx))
    }

    fn icon_row(&mut self, r: usize, cols: usize, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let mut cells: Vec<AnyElement> = Vec::with_capacity(cols);
        for row in r * cols..(r + 1) * cols {
            let Some(entry) = self.tab().entry_at(row).cloned() else {
                break;
            };
            let selected = self.tab().selection.contains(row);
            let (icon, color) = file_icon(&entry, &t);
            let thumb = af_fs::kind::is_decodable_image(&entry.name)
                && entry.size > 0
                && entry.size <= THUMBNAIL_MAX_BYTES;
            let tile: AnyElement = if thumb {
                img(entry.path.clone())
                    .size(ICON_TILE)
                    .rounded(RADIUS_SM)
                    .object_fit(ObjectFit::Contain)
                    .into_any_element()
            } else {
                Icon::new(icon)
                    .size(px(40.))
                    .text_color(color)
                    .into_any_element()
            };
            let ws = cx.entity().downgrade();
            let payload = self.drag_payload(row, cx);
            let renaming = self
                .ops
                .renaming
                .as_ref()
                .is_some_and(|r| r.path == entry.path);
            let mut cell = v_flex();
            if is_folder(&entry) {
                cell = dnd::drop_target(cell, entry.path.clone(), ws.clone(), cx);
            }
            let cell = cell.id(("icon", row));
            cells.push(
                dnd::drag_source(cell, payload.filter(|_| !renaming))
                    .flex_none()
                    .w(px(ICON_CELL_WIDTH))
                    .h(px(ICON_CELL_HEIGHT))
                    .p(SPACE_2)
                    .items_center()
                    .gap(SPACE_2)
                    .when(entry.hidden, |d| d.opacity(0.6))
                    .child(
                        div()
                            .size(px(68.))
                            .flex_none()
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_LG)
                            .when_else(
                                selected,
                                |d| d.bg(t.selected),
                                |d| d.hover(|s| s.bg(t.hover)),
                            )
                            .child(tile),
                    )
                    .child(
                        div()
                            .max_w_full()
                            .px(SPACE_2)
                            .rounded(RADIUS_SM)
                            .text_size(FONT_SMALL)
                            .text_center()
                            .truncate()
                            .when(selected, |d| d.bg(t.accent).text_color(t.accent_fg))
                            .child(entry.name.clone()),
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
                        cx.listener(move |ws, _: &MouseDownEvent, _, cx| {
                            ws.right_click_row(row, cx)
                        }),
                    )
                    .on_click(cx.listener(move |ws, e: &ClickEvent, _, cx| {
                        cx.stop_propagation();
                        if e.click_count() >= 2 {
                            ws.open_row(row, cx);
                        } else {
                            ws.release_row(row, cx);
                        }
                    }))
                    .context_menu(move |menu, _, cx| menus::item_menu(menu, &ws, cx))
                    .into_any_element(),
            );
        }
        h_flex()
            .id(("icon-row", r))
            .w_full()
            .h(px(ICON_CELL_HEIGHT))
            .px(SPACE_6)
            .children(cells)
            .into_any_element()
    }
}
