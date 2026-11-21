//! Theme and density: resolved from settings and the system appearance,
//! applied through `af_ui` (aturfile tokens plus GPUI Kit's theme).

use af_ui::catalog::{self, ThemeId};
use af_ui::metrics::Density;
use gpui_kit::*;

pub struct Appearance {
    pub theme: ThemeId,
    pub density: Density,
    /// The window's appearance is dark (what `System` follows).
    pub system_dark: bool,
}

impl Global for Appearance {}

pub fn current(cx: &App) -> &Appearance {
    cx.global::<Appearance>()
}

pub fn density(cx: &App) -> Density {
    cx.try_global::<Appearance>()
        .map(|a| a.density)
        .unwrap_or_default()
}

pub fn is_dark(appearance: WindowAppearance) -> bool {
    matches!(
        appearance,
        WindowAppearance::Dark | WindowAppearance::VibrantDark
    )
}

pub fn init(theme: ThemeId, density: Density, cx: &mut App) {
    let system_dark = is_dark(cx.window_appearance());
    cx.set_global(Appearance {
        theme,
        density,
        system_dark,
    });
    apply(cx);
}

fn apply(cx: &mut App) {
    let a = current(cx);
    let palette = catalog::resolve(&a.theme, a.system_dark);
    af_ui::tokens::apply_palette(&palette, cx);
    af_ui::fonts::apply(cx);
    cx.refresh_windows();
}

pub fn set_theme(theme: ThemeId, cx: &mut App) {
    cx.global_mut::<Appearance>().theme = theme;
    apply(cx);
}

pub fn set_density(density: Density, cx: &mut App) {
    cx.global_mut::<Appearance>().density = density;
    cx.refresh_windows();
}

/// The system switched between light and dark.
pub fn set_system_dark(dark: bool, cx: &mut App) {
    let a = cx.global_mut::<Appearance>();
    if a.system_dark == dark {
        return;
    }
    a.system_dark = dark;
    if a.theme == ThemeId::System {
        apply(cx);
    }
}

/// The name shown for the active theme ("System (aturfile Dark)").
pub fn theme_label(cx: &App) -> String {
    let a = current(cx);
    match &a.theme {
        ThemeId::System => {
            let name = if a.system_dark {
                catalog::DEFAULT_DARK
            } else {
                catalog::DEFAULT_LIGHT
            };
            format!("System ({name})")
        }
        ThemeId::Named(n) => n.clone(),
    }
}
