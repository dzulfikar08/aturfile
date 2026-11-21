//! Small modal sheets over the window: About, and Pro (in App Store builds,
//! shown when a Pro feature is used).

use af_ui::assets::APP_ICON;
use af_ui::metrics::*;
use af_ui::tokens::{Elevation, tokens};
use gpui_kit::assets::IconName;
use gpui_kit::component::button::{Button, ButtonVariants as _};
use gpui_kit::component::{Disableable as _, Icon, Sizable as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Workspace;
use crate::pro;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Sheet {
    About,
    Pro,
}

/// What Pro adds in App Store builds.
const PRO_FEATURES: &[&str] = &[
    "Tabs: keep several folders open in one window",
    "Clean Up: move what you staged to the Trash in one go",
    "Supports an independent developer and future updates",
];

impl Workspace {
    pub(crate) fn open_sheet(&mut self, sheet: Sheet, cx: &mut Context<Self>) {
        if sheet == Sheet::Pro {
            pro::refresh(cx);
        }
        self.sheet = Some(sheet);
        cx.notify();
    }

    pub(crate) fn close_sheet(&mut self, cx: &mut Context<Self>) {
        if self.sheet.take().is_some() {
            cx.notify();
        }
    }

    /// Pro features call this: true when they may go ahead, otherwise the
    /// Pro sheet opens.
    pub(crate) fn require_pro(&mut self, cx: &mut Context<Self>) -> bool {
        if pro::unlocked(cx) {
            return true;
        }
        self.open_sheet(Sheet::Pro, cx);
        false
    }

    pub(super) fn render_sheet(&mut self, cx: &mut Context<Self>) -> Option<AnyElement> {
        let sheet = self.sheet?;
        let t = tokens(cx);
        let body = match sheet {
            Sheet::About => self.about_body(cx).into_any_element(),
            Sheet::Pro => self.pro_body(cx).into_any_element(),
        };
        Some(
            div()
                .id("sheet-scrim")
                .absolute()
                .inset_0()
                .flex()
                .items_center()
                .justify_center()
                .bg(t.scrim)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _: &MouseDownEvent, _, cx| ws.close_sheet(cx)),
                )
                .child(
                    v_flex()
                        .id("sheet")
                        .w(px(380.))
                        .p(SPACE_9)
                        .gap(SPACE_6)
                        .items_center()
                        .rounded(RADIUS_XL)
                        .bg(t.elevated)
                        .border_1()
                        .border_color(t.border_strong)
                        .shadow(t.shadow(Elevation::Modal))
                        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                        .child(img(APP_ICON).size(px(88.)))
                        .child(body),
                )
                .into_any_element(),
        )
    }

    fn about_body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let edition = if af_store::STORE_BUILD {
            if pro::unlocked(cx) { "Pro" } else { "Free" }
        } else {
            "Open source build: everything unlocked"
        };
        v_flex()
            .items_center()
            .gap(SPACE_3)
            .child(
                div()
                    .text_size(FONT_DISPLAY)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child("aturfile"),
            )
            .child(
                div()
                    .text_size(FONT_SMALL)
                    .text_color(t.fg_muted)
                    .child(format!("Version {}  \u{00b7}  {edition}", env!("CARGO_PKG_VERSION"))),
            )
            .child(
                div()
                    .pt(SPACE_3)
                    .text_center()
                    .text_size(FONT_SMALL)
                    .text_color(t.fg_muted)
                    .child("A fast file manager with disk cleanup. Free software under the GPL 3.0 or later."),
            )
            .child(
                h_flex()
                    .pt(SPACE_5)
                    .gap(SPACE_4)
                    .child(
                        Button::new("about-site")
                            .small()
                            .outline()
                            .label("Source code")
                            .on_click(|_, _, cx| cx.open_url("https://github.com/dzulfikar08/aturfile")),
                    )
                    .child(
                        Button::new("about-close")
                            .small()
                            .primary()
                            .label("Close")
                            .on_click(cx.listener(|ws, _, _, cx| ws.close_sheet(cx))),
                    ),
            )
    }

    fn pro_body(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let (unlocked, busy, price, message) = match pro::state(cx) {
            Some(p) => (p.unlocked, p.busy, p.price.clone(), p.message.clone()),
            None => (true, false, None, None),
        };
        let title = if unlocked {
            "You have aturfile Pro"
        } else {
            "aturfile Pro"
        };
        let buy_label = match &price {
            Some(p) => format!("Unlock Pro for {p}"),
            None => "Unlock Pro".to_string(),
        };
        v_flex()
            .w_full()
            .items_center()
            .gap(SPACE_4)
            .child(
                div()
                    .text_size(FONT_DISPLAY)
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                v_flex().w_full().gap(SPACE_3).pt(SPACE_2).children(PRO_FEATURES.iter().map(|f| {
                    h_flex()
                        .gap(SPACE_4)
                        .text_size(FONT_SMALL)
                        .child(Icon::new(IconName::CircleCheck).size(ICON_MD).text_color(t.success))
                        .child(div().flex_1().child(*f))
                })),
            )
            .child(
                div()
                    .text_size(FONT_CAPTION)
                    .text_color(t.fg_dim)
                    .text_center()
                    .child("One purchase, no subscription. Browsing, search, previews and Clean Up's scan stay free."),
            )
            .children(message.map(|m| {
                div().text_size(FONT_SMALL).text_color(t.warning).text_center().child(m)
            }))
            .child(
                h_flex()
                    .pt(SPACE_4)
                    .gap(SPACE_4)
                    .when(!unlocked, |d| {
                        d.child(
                            Button::new("pro-restore")
                                .small()
                                .outline()
                                .label("Restore")
                                .disabled(busy)
                                .on_click(|_, _, cx| pro::restore(cx)),
                        )
                        .child(
                            Button::new("pro-buy")
                                .small()
                                .primary()
                                .label(buy_label)
                                .loading(busy)
                                .disabled(busy)
                                .on_click(|_, _, cx| pro::purchase(cx)),
                        )
                    })
                    .child(
                        Button::new("pro-close")
                            .small()
                            .ghost()
                            .label(if unlocked { "Close" } else { "Not now" })
                            .on_click(cx.listener(|ws, _, _, cx| ws.close_sheet(cx))),
                    ),
            )
    }
}
