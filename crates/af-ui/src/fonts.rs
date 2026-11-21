//! UI and monospace font selection. aturfile never bundles fonts: it uses the
//! system's (SF Pro and SF Mono on macOS) or the best installed alternative.

use gpui_kit::component::Theme;
use gpui_kit::*;
use std::sync::OnceLock;

/// Preferred UI fonts, best first. Empty match keeps GPUI Kit's default,
/// which is the system font on macOS.
pub const UI_FONTS: &[&str] = &["SF Pro Text", "SF Pro", "Inter", "Noto Sans"];

/// Preferred monospace fonts, best first.
pub const MONO_FONTS: &[&str] = &[
    "SF Mono",
    "JetBrains Mono",
    "Menlo",
    "Noto Sans Mono",
    "DejaVu Sans Mono",
];

fn first_installed(installed: &[String], prefs: &[&str]) -> Option<String> {
    prefs
        .iter()
        .find(|p| installed.iter().any(|f| f == *p))
        .map(|p| (*p).to_owned())
}

/// Installed font families, read once per run: listing them takes about
/// 150 ms on macOS, too slow to repeat on every theme change.
fn installed(cx: &App) -> &'static [String] {
    static FONTS: OnceLock<Vec<String>> = OnceLock::new();
    FONTS.get_or_init(|| cx.text_system().all_font_names())
}

/// Apply the best UI and monospace fonts to the theme (again after every
/// palette change: a theme mode switch resets them).
pub fn apply(cx: &mut App) {
    let fonts = installed(cx);
    let ui = first_installed(fonts, UI_FONTS);
    let mono = first_installed(fonts, MONO_FONTS);
    Theme::update(cx, |theme| {
        if let Some(ui) = ui {
            theme.font_family = ui.into();
        }
        if let Some(mono) = mono {
            theme.mono_font_family = mono.into();
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_the_first_installed() {
        let installed = vec!["Menlo".to_owned(), "Inter".to_owned()];
        assert_eq!(
            first_installed(&installed, MONO_FONTS).as_deref(),
            Some("Menlo")
        );
        assert_eq!(first_installed(&[], MONO_FONTS), None);
    }
}
