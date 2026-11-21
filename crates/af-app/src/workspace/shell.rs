//! Title bar with tabs, the Files page layout, status bar and the sandbox
//! grant screen.

use std::path::Path;

use af_ui::components::{IconButtonExt as _, icon_button, kbd};
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Sizable as _, TitleBar, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::{Drag, Workspace};
use crate::actions::*;
use crate::format;
use crate::tab::{Page, ViewMode};

/// Below this window width the title bar drops text labels.
const COMPACT_WINDOW: Pixels = px(860.);

impl Workspace {
    pub(super) fn render_title_bar(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = tokens(cx);
        let compact = window.viewport_size().width < COMPACT_WINDOW;

        let sidebar_toggle = icon_button("toggle-sidebar", IconName::PanelLeft, false, cx)
            .tip(
                if self.settings.show_sidebar {
                    "Hide sidebar"
                } else {
                    "Show sidebar"
                },
                Some("Cmd-Alt-S"),
            )
            .on_click(cx.listener(|ws, _, _, cx| ws.toggle_sidebar_panel(cx)));

        let mut tabs = Vec::with_capacity(self.tabs.len());
        let many = self.tabs.len() > 1;
        for (ix, tab) in self.tabs.iter().enumerate() {
            let selected = ix == self.active;
            let group = SharedString::from(format!("tab-{}", tab.id));
            let icon = match tab.page {
                Page::Cleanup => IconName::BrushCleaning,
                Page::Files => IconName::Folder,
            };
            tabs.push(
                div()
                    .id(("tab", tab.id as usize))
                    .group(group.clone())
                    .flex_none()
                    .h(TAB_HEIGHT)
                    .pl(SPACE_4)
                    .pr(SPACE_2)
                    .flex()
                    .items_center()
                    .gap(SPACE_3)
                    .rounded(RADIUS_MD)
                    .text_size(FONT_SMALL)
                    .font_weight(FontWeight::MEDIUM)
                    .cursor_pointer()
                    .when_else(
                        selected,
                        |d| {
                            d.bg(t.bg)
                                .border_1()
                                .border_color(t.border_strong)
                                .text_color(t.fg)
                        },
                        |d| {
                            d.border_1()
                                .border_color(transparent_black())
                                .text_color(t.fg_muted)
                                .hover(|s| s.bg(t.hover))
                        },
                    )
                    .child(
                        gpui_kit::component::Icon::new(icon)
                            .size(ICON_SM)
                            .text_color(if selected { t.kind_folder } else { t.fg_dim }),
                    )
                    .child(
                        div()
                            .w(if compact { px(84.) } else { TAB_TITLE_WIDTH })
                            .truncate()
                            .child(tab.title()),
                    )
                    .child(
                        div()
                            .id(("tab-close", tab.id as usize))
                            .size(px(18.))
                            .flex()
                            .items_center()
                            .justify_center()
                            .rounded(RADIUS_SM)
                            .text_color(t.fg_dim)
                            .hover(|s| s.bg(t.hover).text_color(t.fg))
                            .when(!(selected && many), |d| {
                                d.invisible().group_hover(group, |s| s.visible())
                            })
                            .child(gpui_kit::component::Icon::new(IconName::X).size(ICON_SM))
                            .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                                cx.stop_propagation();
                                ws.close_tab(ix, window, cx);
                            })),
                    )
                    .on_click(
                        cx.listener(move |ws, _: &ClickEvent, _, cx| ws.activate_tab(ix, cx)),
                    ),
            );
        }

        let new_tab = icon_button("tab-new", IconName::Plus, false, cx)
            .tip("New tab", Some("Cmd-T"))
            .on_click(cx.listener(|ws, _, window, cx| ws.on_new_tab(&NewTab, window, cx)));

        let palette_hint = div()
            .id("palette-hint")
            .flex_none()
            .h(TAB_HEIGHT)
            .px(SPACE_4)
            .flex()
            .items_center()
            .gap(SPACE_3)
            .rounded(RADIUS_MD)
            .text_size(FONT_SMALL)
            .text_color(t.fg_muted)
            .cursor_pointer()
            .hover(|s| s.bg(t.hover))
            .when(!compact, |d| d.child("Commands"))
            .child(kbd("Cmd K", cx))
            .on_click(cx.listener(|ws, _: &ClickEvent, window, cx| {
                ws.on_toggle_palette(&TogglePalette, window, cx)
            }));

        TitleBar::new()
            .h(TITLE_BAR_HEIGHT)
            .bg(t.chrome)
            .border_color(t.border)
            .child(
                h_flex()
                    .size_full()
                    .pr(SPACE_4)
                    .gap(SPACE_2)
                    .child(sidebar_toggle)
                    .child(
                        h_flex()
                            .id("tab-list")
                            .flex_1()
                            .min_w_0()
                            .gap(SPACE_2)
                            .overflow_x_scroll()
                            .children(tabs)
                            .child(new_tab),
                    )
                    .child(palette_hint),
            )
    }

    /// Toolbar, the active view and the preview pane.
    pub(super) fn render_files(&mut self, window: &mut Window, cx: &mut Context<Self>) -> Div {
        let t = tokens(cx);
        let preview_open = self.settings.show_preview;
        let preview_w = px(self.settings.preview_width);
        let covered = self.grants.covers(&self.tab().cwd);
        let view = if !covered {
            self.render_grant(&self.tab().cwd.clone(), cx)
        } else if let Some(error) = self.tab().error.clone() {
            div()
                .size_full()
                .flex()
                .items_center()
                .justify_center()
                .p(SPACE_9)
                .text_color(t.danger)
                .child(error)
                .into_any_element()
        } else {
            match self.tab().view {
                ViewMode::List => self.render_list(cx).into_any_element(),
                ViewMode::Columns => self.render_columns(window, cx).into_any_element(),
                ViewMode::Icons => self.render_icons(window, cx).into_any_element(),
            }
        };
        let show_preview = preview_open && covered && self.tab().view != ViewMode::Columns;
        if show_preview {
            self.sync_preview(cx);
        }
        v_flex().size_full().child(self.render_toolbar(cx)).child(
            h_flex()
                .flex_1()
                .min_h_0()
                .w_full()
                .child(div().flex_1().min_w_0().h_full().child(view))
                .when(show_preview, |d| {
                    d.child(
                        div()
                            .relative()
                            .flex_none()
                            .h_full()
                            .w(preview_w)
                            .border_l_1()
                            .border_color(t.border)
                            .bg(t.chrome)
                            .child(self.render_preview(cx))
                            .child(
                                self.resize_handle("preview-edge", Drag::Preview, cx)
                                    .left(-RESIZE_HANDLE / 2.),
                            ),
                    )
                }),
        )
    }

    pub(super) fn render_status_bar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let tab = self.tab();
        let mut left: Vec<AnyElement> = Vec::new();
        match tab.page {
            Page::Files => {
                let shown = tab.visible.len();
                let total = tab.entries.len();
                let mut text = format::items(shown);
                if shown != total && !tab.query.is_empty() {
                    text = format!(
                        "{shown} of {total} items match \u{201c}{}\u{201d}",
                        tab.query
                    );
                }
                left.push(div().text_color(t.fg).child(text).into_any_element());
                let n = tab.selection.count();
                if n > 0 {
                    let bytes = tab.selected_bytes();
                    let mut sel = format!("{n} selected");
                    if bytes > 0 {
                        sel.push_str(&format!(", {}", af_fs::format_size(bytes)));
                    }
                    left.push(div().child(sel).into_any_element());
                }
                if self.show_hidden {
                    left.push(
                        div()
                            .text_color(t.fg_dim)
                            .child("Hidden files shown")
                            .into_any_element(),
                    );
                }
            }
            Page::Cleanup => {
                left.push(div().text_color(t.fg).child("Clean Up").into_any_element());
            }
        }
        let progress = self.render_progress(cx);
        let notice = self.notice.clone();
        let free = self.volume_of(&tab.cwd).map(|v| {
            format!(
                "{} free of {} on {}",
                af_fs::format_size(v.free),
                af_fs::format_size(v.total),
                v.name
            )
        });
        let view_label = match tab.page {
            Page::Files => tab.view.label(),
            Page::Cleanup => "Clean Up",
        };
        h_flex()
            .flex_none()
            .w_full()
            .h(STATUS_BAR_HEIGHT)
            .px(SPACE_6)
            .gap(SPACE_6)
            .bg(t.chrome)
            .border_t_1()
            .border_color(t.border)
            .text_size(FONT_CAPTION)
            .text_color(t.fg_muted)
            .children(left)
            .child(div().flex_1().min_w_0().truncate().children(notice))
            .children(progress)
            .children(free.map(|f| div().flex_none().child(f)))
            .child(
                div()
                    .flex_none()
                    .px(SPACE_3)
                    .rounded(RADIUS_SM)
                    .bg(t.surface)
                    .child(view_label),
            )
    }

    /// The mounted volume holding `path` (the deepest mount point).
    pub(crate) fn volume_of(&self, path: &Path) -> Option<&af_mac::Volume> {
        self.volumes
            .iter()
            .filter(|v| path.starts_with(&v.path))
            .max_by_key(|v| v.path.as_os_str().len())
    }

    pub(crate) fn render_grant(&self, folder: &Path, cx: &mut Context<Self>) -> AnyElement {
        let t = tokens(cx);
        let home = af_clean::paths::home();
        let name = if folder == home {
            "your Home folder".to_string()
        } else {
            af_fs::location::display(folder, &home)
        };
        v_flex()
            .size_full()
            .items_center()
            .justify_center()
            .gap(SPACE_6)
            .px(SPACE_10)
            .child(
                div()
                    .size(px(56.))
                    .rounded(RADIUS_XL)
                    .bg(t.accent_soft)
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        gpui_kit::component::Icon::new(IconName::FolderOpen)
                            .size(px(28.))
                            .text_color(t.accent),
                    ),
            )
            .child(
                div()
                    .text_size(FONT_TITLE)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(format!("Allow aturfile to open {name}")),
            )
            .child(
                div()
                    .max_w(px(440.))
                    .text_center()
                    .text_color(t.fg_muted)
                    .child(
                        "macOS asks you to choose each folder an App Store app may open. \
                         Choose your Home folder once to browse and clean everything in it.",
                    ),
            )
            .child(
                Button::new("grant")
                    .primary()
                    .small()
                    .label("Choose Folder\u{2026}")
                    .on_click(cx.listener(|ws, _, _, cx| ws.request_access(cx))),
            )
            .into_any_element()
    }
}
