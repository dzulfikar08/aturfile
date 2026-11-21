//! The bundled themes. macOS has no Omarchy to follow, so aturfile ships a
//! small curated set: its own dark and light themes plus popular schemes from
//! iTerm2-Color-Schemes (MIT, `assets/themes.tsv`, the table Kantor uses).

use crate::palette::{Palette, Rgb, luminance, mix, parse_hex};

const THEMES_TSV: &str = include_str!("../assets/themes.tsv");

pub const DEFAULT_DARK: &str = "aturfile Dark";
pub const DEFAULT_LIGHT: &str = "aturfile Light";

/// What the user picked, stored in settings as a string.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum ThemeId {
    /// aturfile Dark or Light, following the system appearance.
    System,
    Named(String),
}

impl ThemeId {
    /// `system` or a theme name; anything unknown falls back at resolve time.
    pub fn parse(s: &str) -> Self {
        let s = s.trim();
        if s.is_empty() || s.eq_ignore_ascii_case("system") {
            Self::System
        } else {
            Self::Named(s.to_owned())
        }
    }

    pub fn to_setting(&self) -> String {
        match self {
            Self::System => "system".to_owned(),
            Self::Named(n) => n.clone(),
        }
    }
}

#[derive(Debug, Clone)]
pub struct ThemeEntry {
    pub name: String,
    pub palette: Palette,
}

impl ThemeEntry {
    /// Colours for a swatch strip.
    pub fn swatch(&self) -> [Rgb; 6] {
        let p = &self.palette;
        [p.red, p.green, p.yellow, p.blue, p.magenta, p.cyan]
    }
}

/// A palette from a terminal scheme (`bg fg selection p0..p15`).
fn scheme_palette(bg: Rgb, fg: Rgb, sel: Rgb, pal: &[Rgb; 16]) -> Palette {
    Palette {
        dark: luminance(bg) < 0.5,
        background: bg,
        foreground: fg,
        accent: pal[4],
        selection: sel,
        muted: mix(fg, bg, 0.45),
        red: pal[1],
        green: pal[2],
        yellow: pal[3],
        blue: pal[4],
        magenta: pal[5],
        cyan: pal[6],
    }
}

fn bundled_schemes() -> impl Iterator<Item = ThemeEntry> {
    THEMES_TSV
        .lines()
        .filter(|l| !l.starts_with('#') && !l.is_empty())
        .filter_map(|line| {
            let mut f = line.split('\t');
            let name = f.next()?.to_owned();
            let cols: Vec<Rgb> = f.map(parse_hex).collect::<Option<_>>()?;
            if cols.len() != 19 {
                return None;
            }
            let pal: [Rgb; 16] = cols[3..19].try_into().ok()?;
            Some(ThemeEntry {
                palette: scheme_palette(cols[0], cols[1], cols[2], &pal),
                name,
            })
        })
}

/// Every bundled theme, aturfile's own first, then by name.
pub fn all() -> Vec<ThemeEntry> {
    let mut schemes: Vec<ThemeEntry> = bundled_schemes().collect();
    schemes.sort_by_key(|e| e.name.to_lowercase());
    let mut v = vec![
        ThemeEntry {
            name: DEFAULT_DARK.to_owned(),
            palette: Palette::default(),
        },
        ThemeEntry {
            name: DEFAULT_LIGHT.to_owned(),
            palette: Palette::light(),
        },
    ];
    v.extend(schemes);
    v
}

/// The palette for `id`; `System` and unknown names follow `dark`.
pub fn resolve(id: &ThemeId, dark: bool) -> Palette {
    let fallback = || {
        if dark {
            Palette::default()
        } else {
            Palette::light()
        }
    };
    match id {
        ThemeId::System => fallback(),
        ThemeId::Named(name) => all()
            .into_iter()
            .find(|e| e.name.eq_ignore_ascii_case(name))
            .map(|e| e.palette)
            .unwrap_or_else(fallback),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_parses() {
        let themes = all();
        assert!(themes.len() >= 15, "only {} themes", themes.len());
        assert_eq!(themes[0].name, DEFAULT_DARK);
        let dracula = themes.iter().find(|t| t.name == "Dracula").unwrap();
        assert!(dracula.palette.dark);
        assert_eq!(dracula.palette.background, 0x282a36);
        let latte = themes
            .iter()
            .find(|t| t.name == "Catppuccin Latte")
            .unwrap();
        assert!(!latte.palette.dark);
    }

    #[test]
    fn ids_round_trip() {
        for id in [ThemeId::System, ThemeId::Named("Nord".into())] {
            assert_eq!(ThemeId::parse(&id.to_setting()), id);
        }
        assert_eq!(ThemeId::parse(""), ThemeId::System);
    }

    #[test]
    fn system_follows_appearance() {
        assert!(resolve(&ThemeId::System, true).dark);
        assert!(!resolve(&ThemeId::System, false).dark);
        // Unknown names fall back the same way.
        assert!(!resolve(&ThemeId::Named("No Such".into()), false).dark);
        assert_eq!(
            resolve(&ThemeId::Named("nord".into()), false).background,
            all()
                .iter()
                .find(|t| t.name == "Nord")
                .unwrap()
                .palette
                .background
        );
    }
}
