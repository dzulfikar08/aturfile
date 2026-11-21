//! The toolbar: back/forward, the breadcrumb path bar (click empty space or
//! Cmd-L to type a location), view switcher, filter field, preview toggle.

use af_ui::components::{IconButtonExt as _, icon_button};
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{Escape, Input};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex};
use gpui_kit::*;

use super::Workspace;
use crate::tab::ViewMode;

impl Workspace {
    pub(super) fn render_toolbar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let tab = self.tab();
        let (can_back, can_forward, view) =
            (!tab.back.is_empty(), !tab.forward.is_empty(), tab.view);

        let back = icon_button("back", IconName::ChevronLeft, false, cx)
            .tip("Back", Some("Cmd-["))
            .disabled(!can_back)
            .on_click(cx.listener(|ws, _, _, cx| ws.back(cx)));
        let forward = icon_button("forward", IconName::ChevronRight, false, cx)
            .tip("Forward", Some("Cmd-]"))
            .disabled(!can_forward)
            .on_click(cx.listener(|ws, _, _, cx| ws.forward(cx)));

        let path_bar = if self.editing_location {
            div()
                .id("location")
                .key_context("Location")
                .flex_1()
                .min_w_0()
                .on_action(
                    cx.listener(|ws, _: &Escape, window, cx| ws.stop_editing_location(window, cx)),
                )
                .child(
                    Input::new(&self.location).small().prefix(
                        Icon::new(IconName::Folder)
                            .size(ICON_SM)
                            .text_color(t.fg_dim),
                    ),
                )
                .into_any_element()
        } else {
            self.render_crumbs(cx).into_any_element()
        };

        let view_button = |id: &'static str,
                           icon: IconName,
                           mode: ViewMode,
                           tip: &str,
                           keys: &str,
                           cx: &mut Context<Self>| {
            icon_button(id, icon, view == mode, cx)
                .tip(tip, Some(keys))
                .on_click(cx.listener(move |ws, _, _, cx| ws.set_view(mode, cx)))
        };
        let switcher = h_flex()
            .flex_none()
            .p(px(2.))
            .gap(px(2.))
            .rounded(RADIUS_LG)
            .border_1()
            .border_color(t.border)
            .bg(t.surface)
            .child(view_button(
                "view-list",
                IconName::List,
                ViewMode::List,
                "List",
                "Cmd-1",
                cx,
            ))
            .child(view_button(
                "view-columns",
                IconName::Columns3,
                ViewMode::Columns,
                "Columns",
                "Cmd-2",
                cx,
            ))
            .child(view_button(
                "view-icons",
                IconName::LayoutGrid,
                ViewMode::Icons,
                "Icons",
                "Cmd-3",
                cx,
            ));

        let filter = div()
            .id("filter")
            .key_context("Filter")
            .flex_none()
            .w(px(188.))
            .child(
                Input::new(&self.filter).small().cleanable(true).prefix(
                    Icon::new(IconName::Search)
                        .size(ICON_SM)
                        .text_color(t.fg_dim),
                ),
            );

        let preview = icon_button(
            "toggle-preview",
            IconName::PanelRight,
            self.settings.show_preview,
            cx,
        )
        .tip(
            if self.settings.show_preview {
                "Hide preview"
            } else {
                "Show preview"
            },
            Some("Cmd-Alt-P"),
        )
        .on_click(cx.listener(|ws, _, _, cx| ws.toggle_preview_panel(cx)));

        h_flex()
            .flex_none()
            .w_full()
            .h(TOOLBAR_HEIGHT)
            .px(SPACE_5)
            .gap(SPACE_3)
            .border_b_1()
            .border_color(t.border)
            .child(back)
            .child(forward)
            .child(path_bar)
            .child(switcher)
            .child(filter)
            .child(preview)
    }

    /// The path as clickable segments; empty space starts typing a location.
    fn render_crumbs(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let home = af_clean::paths::home();
        let crumbs =
            af_fs::location::collapse(af_fs::location::breadcrumbs(&self.tab().cwd, &home), 4);
        let last = crumbs.len().saturating_sub(1);
        let mut row = h_flex()
            .id("crumbs")
            .flex_1()
            .min_w_0()
            .h(CONTROL_MD)
            .px(SPACE_3)
            .gap(SPACE_1)
            .rounded(RADIUS_MD)
            .overflow_hidden()
            .cursor_text()
            .hover(|s| s.bg(t.hover))
            .on_click(
                cx.listener(|ws, _: &ClickEvent, window, cx| ws.start_editing_location(window, cx)),
            );
        for (i, crumb) in crumbs.into_iter().enumerate() {
            if i > 0 {
                row = row.child(
                    Icon::new(IconName::ChevronRight)
                        .size(ICON_SM)
                        .text_color(t.fg_dim)
                        .flex_none(),
                );
            }
            let is_last = i == last;
            let path = crumb.path.clone();
            let label = if crumb.label == "~" {
                "Home".to_string()
            } else if crumb.label == "/" {
                self.volumes
                    .iter()
                    .find(|v| v.is_root)
                    .map(|v| v.name.clone())
                    .unwrap_or_else(|| "/".into())
            } else {
                crumb.label
            };
            row = row.child(
                div()
                    .id(("crumb", i))
                    .flex_shrink()
                    .min_w(px(24.))
                    .px(SPACE_2)
                    .py(px(1.))
                    .rounded(RADIUS_SM)
                    .truncate()
                    .cursor_pointer()
                    .text_size(FONT_BODY)
                    .when_else(
                        is_last,
                        |d| d.text_color(t.fg).font_weight(FontWeight::MEDIUM),
                        |d| d.text_color(t.fg_muted),
                    )
                    .hover(|s| s.bg(t.hover).text_color(t.fg))
                    .child(label)
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        cx.stop_propagation();
                        ws.go(path.clone(), None, cx);
                        ws.focus_list(window, cx);
                    })),
            );
        }
        row
    }
}
