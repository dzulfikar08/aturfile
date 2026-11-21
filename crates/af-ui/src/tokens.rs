//! Colour tokens derived from a [`Palette`] (Kantor's recipe): chrome,
//! surfaces and borders are the foreground blended over the background at
//! small fixed amounts, so every theme yields a consistent, layered UI.

use gpui_kit::component::{Theme, ThemeMode};
use gpui_kit::*;

use crate::palette::{Palette, Rgb, contrast, mix, readable};

/// Semantic colours for aturfile's own views. Read with [`tokens`].
#[derive(Debug, Clone, Copy)]
pub struct Tokens {
    pub dark: bool,
    /// The file view's background.
    pub bg: Hsla,
    /// Title bar, sidebar, status bar.
    pub chrome: Hsla,
    /// Cards, inputs, table headers.
    pub surface: Hsla,
    /// Menus, dialogs, the command palette.
    pub elevated: Hsla,
    pub fg: Hsla,
    pub fg_muted: Hsla,
    pub fg_dim: Hsla,
    pub border: Hsla,
    pub border_strong: Hsla,
    /// Outlines of controls the user must find (inputs, checkboxes): at least
    /// 3:1 against every surface they sit on (WCAG 1.4.11).
    pub control_border: Hsla,
    pub hover: Hsla,
    /// The selected row while the list has focus.
    pub selected: Hsla,
    /// The selected row in a list that doesn't have focus (Miller columns
    /// to the left of the active one).
    pub selected_inactive: Hsla,
    pub accent: Hsla,
    pub accent_soft: Hsla,
    pub accent_fg: Hsla,
    pub success: Hsla,
    pub warning: Hsla,
    pub danger: Hsla,
    pub info: Hsla,
    pub scrim: Hsla,
    /// File kinds: icon colours, readable (3:1) on `bg` and `chrome`.
    pub kind_folder: Hsla,
    pub kind_image: Hsla,
    pub kind_media: Hsla,
    pub kind_audio: Hsla,
    pub kind_archive: Hsla,
    pub kind_code: Hsla,
    pub kind_document: Hsla,
    pub kind_app: Hsla,
    pub kind_other: Hsla,
    /// Capacity bars: the track and the used part (warning when nearly full).
    pub bar_track: Hsla,
    pub bar_fill: Hsla,
}

impl Global for Tokens {}

/// The active tokens (defaults until [`apply_palette`] runs).
pub fn tokens(cx: &App) -> Tokens {
    cx.try_global::<Tokens>()
        .copied()
        .unwrap_or_else(|| Tokens::from_palette(&Palette::default()))
}

fn c(rgb: Rgb) -> Hsla {
    gpui_kit::rgb(rgb).into()
}

fn alpha(rgb: Rgb, a: f32) -> Hsla {
    let mut h = c(rgb);
    h.a = a;
    h
}

/// The palette's foreground blended over its background, the least that
/// reaches `ratio` against every colour of `on` (all of them when it can't).
fn contrasting(p: &Palette, on: &[Rgb], ratio: f32) -> Rgb {
    (10..=100)
        .map(|k| mix(p.background, p.foreground, k as f32 / 100.0))
        .find(|c| on.iter().all(|s| contrast(*c, *s) >= ratio))
        .unwrap_or(p.foreground)
}

fn luminance(rgb: Rgb) -> f32 {
    crate::palette::luminance(rgb)
}

#[derive(Debug, Clone, Copy)]
pub enum Elevation {
    Raised,
    Popover,
    Modal,
}

impl Tokens {
    pub fn from_palette(p: &Palette) -> Self {
        let on_bg = |a: f32| mix(p.background, p.foreground, a);
        let surfaces: Vec<Rgb> = if p.dark {
            [0.0, 0.03, 0.05, 0.07].iter().map(|a| on_bg(*a)).collect()
        } else {
            [0.0, 0.025, 0.035]
                .iter()
                .map(|a| on_bg(*a))
                .chain([0xffffff])
                .collect()
        };
        let control_border = c(contrasting(p, &surfaces, 3.1));
        let (chrome, surface, elevated, border, border_strong) = if p.dark {
            (
                on_bg(0.03),
                on_bg(0.05),
                on_bg(0.07),
                on_bg(0.10),
                on_bg(0.18),
            )
        } else {
            (
                on_bg(0.025),
                on_bg(0.035),
                0xffffff,
                on_bg(0.11),
                on_bg(0.20),
            )
        };
        // Icons sit on the view and on the sidebar: readable on the darker
        // (light themes) or lighter (dark themes) of the two.
        let icon_bg = if p.dark { chrome } else { on_bg(0.035) };
        let kind = |rgb: Rgb| c(readable(rgb, icon_bg, p.foreground, 3.0));
        Self {
            dark: p.dark,
            bg: c(p.background),
            chrome: c(chrome),
            surface: c(surface),
            elevated: c(elevated),
            fg: c(p.foreground),
            fg_muted: alpha(p.foreground, 0.68),
            fg_dim: alpha(p.foreground, 0.45),
            border: c(border),
            border_strong: c(border_strong),
            control_border,
            hover: alpha(p.foreground, 0.06),
            selected: alpha(p.accent, if p.dark { 0.26 } else { 0.18 }),
            selected_inactive: alpha(p.foreground, if p.dark { 0.10 } else { 0.08 }),
            accent: c(p.accent),
            accent_soft: alpha(p.accent, 0.12),
            accent_fg: if luminance(p.accent) > 0.6 {
                c(0x000000)
            } else {
                c(0xffffff)
            },
            success: c(p.green),
            warning: c(p.yellow),
            danger: c(p.red),
            info: c(p.blue),
            scrim: Hsla {
                h: 0.,
                s: 0.,
                l: 0.,
                a: if p.dark { 0.45 } else { 0.2 },
            },
            kind_folder: kind(p.blue),
            kind_image: kind(p.magenta),
            kind_media: kind(p.red),
            kind_audio: kind(p.cyan),
            kind_archive: kind(p.yellow),
            kind_code: kind(p.green),
            kind_document: kind(mix(p.blue, p.foreground, 0.25)),
            kind_app: kind(p.accent),
            kind_other: alpha(p.foreground, 0.55),
            bar_track: c(on_bg(if p.dark { 0.12 } else { 0.10 })),
            bar_fill: kind(p.accent),
        }
    }

    /// Shadow stack for an elevation (lighter in light mode).
    pub fn shadow(&self, elevation: Elevation) -> Vec<BoxShadow> {
        let k = if self.dark { 1.0 } else { 0.55 };
        let s = |y: f32, blur: f32, a: f32| BoxShadow {
            color: Hsla {
                h: 0.,
                s: 0.,
                l: 0.,
                a: a * k,
            },
            offset: point(px(0.), px(y)),
            blur_radius: px(blur),
            spread_radius: px(0.),
            inset: false,
        };
        match elevation {
            Elevation::Raised => vec![s(1., 2., 0.18)],
            Elevation::Popover => vec![s(2., 6., 0.22), s(8., 24., 0.18)],
            Elevation::Modal => vec![s(4., 12., 0.25), s(16., 48., 0.30)],
        }
    }
}

/// Install `p` as the active palette: aturfile tokens plus GPUI Kit's theme.
pub fn apply_palette(p: &Palette, cx: &mut App) {
    let t = Tokens::from_palette(p);
    cx.set_global(t);

    let mode = if p.dark {
        ThemeMode::Dark
    } else {
        ThemeMode::Light
    };
    // Load the matching base theme first, then override colours in a second
    // update (a mode switch replaces colours).
    Theme::change(mode, None, cx);

    let solid_hover = c(mix(p.background, p.foreground, 0.06));
    Theme::update(cx, |theme| {
        theme.font_size = crate::metrics::REM;
        theme.radius = crate::metrics::RADIUS_MD;
        theme.radius_lg = crate::metrics::RADIUS_XL;
        theme.shadow = true;

        let k = &mut theme.colors;
        k.background = t.bg;
        k.foreground = t.fg;
        k.border = t.border;
        k.input = t.control_border;
        k.ring = t.accent;
        k.caret = t.fg;
        k.selection = alpha(p.accent, 0.35);

        k.primary = t.accent;
        k.primary_hover = c(mix(p.accent, p.foreground, 0.12));
        k.primary_active = c(mix(p.accent, p.background, 0.12));
        k.primary_foreground = t.accent_fg;
        // Buttons keep their own copies of the primary colours.
        k.button_primary = k.primary;
        k.button_primary_hover = k.primary_hover;
        k.button_primary_active = k.primary_active;
        k.button_primary_foreground = k.primary_foreground;
        k.secondary = t.surface;
        k.secondary_hover = solid_hover;
        k.secondary_active = c(mix(p.background, p.foreground, 0.10));
        k.secondary_foreground = t.fg;
        k.muted = t.surface;
        k.muted_foreground = t.fg_muted;
        k.accent = solid_hover;
        k.accent_foreground = t.fg;
        k.popover = t.elevated;
        k.popover_foreground = t.fg;
        k.danger = t.danger;
        k.success = t.success;
        k.warning = t.warning;
        k.info = t.info;
        k.link = t.accent;

        k.title_bar = t.chrome;
        k.title_bar_border = t.border;
        k.status_bar = t.chrome;
        k.status_bar_border = t.border;
        k.tab_bar = t.chrome;
        k.tab = t.chrome;
        k.tab_active = t.bg;
        k.tab_foreground = t.fg_muted;
        k.tab_active_foreground = t.fg;

        k.list = t.bg;
        k.list_head = t.surface;
        k.list_even = t.bg;
        k.list_hover = solid_hover;
        k.list_active = t.selected;
        k.list_active_border = t.accent;

        k.scrollbar = Hsla::transparent_black();
        k.scrollbar_thumb = alpha(p.foreground, 0.22);
        k.scrollbar_thumb_hover = alpha(p.foreground, 0.38);
        k.window_border = t.border;
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::catalog;

    fn rgb_of(h: Hsla) -> Rgb {
        let r = gpui_kit::Rgba::from(h);
        let ch = |v: f32| (v * 255.0).round() as u32;
        (ch(r.r) << 16) | (ch(r.g) << 8) | ch(r.b)
    }

    /// Control outlines stand 3:1 apart from every surface, in every bundled
    /// theme, and stay quieter than text.
    #[std::prelude::v1::test]
    fn control_borders_reach_three_to_one() {
        for theme in catalog::all() {
            let p = &theme.palette;
            let t = Tokens::from_palette(p);
            let border = rgb_of(t.control_border);
            for (name, s) in [
                ("bg", t.bg),
                ("chrome", t.chrome),
                ("surface", t.surface),
                ("elevated", t.elevated),
            ] {
                let r = contrast(border, rgb_of(s));
                assert!(r >= 3.0, "{} {name} {r:.2}", theme.name);
            }
            assert!(contrast(border, p.background) < contrast(p.foreground, p.background));
        }
    }

    /// File-kind icons are visible on the view in every bundled theme.
    #[std::prelude::v1::test]
    fn kind_colours_are_visible() {
        for theme in catalog::all() {
            let t = Tokens::from_palette(&theme.palette);
            for (name, k) in [
                ("folder", t.kind_folder),
                ("image", t.kind_image),
                ("archive", t.kind_archive),
                ("code", t.kind_code),
            ] {
                let r = contrast(rgb_of(k), theme.palette.background);
                assert!(r >= 2.8, "{} {name} {r:.2}", theme.name);
            }
        }
    }
}
