//! Plain RGB palettes (no GPUI types): the input to [`crate::tokens`].

/// A 24-bit RGB colour, `0xRRGGBB`.
pub type Rgb = u32;

/// The colours a theme provides.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Palette {
    pub dark: bool,
    pub background: Rgb,
    pub foreground: Rgb,
    pub accent: Rgb,
    pub selection: Rgb,
    pub muted: Rgb,
    pub red: Rgb,
    pub green: Rgb,
    pub yellow: Rgb,
    pub blue: Rgb,
    pub magenta: Rgb,
    pub cyan: Rgb,
}

impl Default for Palette {
    /// aturfile Dark: neutral graphite with a clear blue accent.
    fn default() -> Self {
        Self {
            dark: true,
            background: 0x1b1c20,
            foreground: 0xe3e5ea,
            accent: 0x4d8dff,
            selection: 0x2c3a55,
            muted: 0x868b96,
            red: 0xf0716b,
            green: 0x4cc38a,
            yellow: 0xe3b04b,
            blue: 0x5b9bff,
            magenta: 0xb68cf5,
            cyan: 0x4cc5c9,
        }
    }
}

impl Palette {
    /// aturfile Light: near-white paper, dark ink, the same blue accent.
    pub fn light() -> Self {
        Self {
            dark: false,
            background: 0xfbfbfc,
            foreground: 0x1d1f24,
            accent: 0x2f6fed,
            selection: 0xd6e3fc,
            muted: 0x6b7180,
            red: 0xd4423b,
            green: 0x1f8f57,
            yellow: 0xb0761a,
            blue: 0x2f6fed,
            magenta: 0x8a4fd8,
            cyan: 0x13878f,
        }
    }
}

/// Parse `#rrggbb` / `rrggbb` (an optional alpha byte is ignored).
pub fn parse_hex(s: &str) -> Option<Rgb> {
    let h = s.trim().trim_start_matches('#');
    let h = match h.len() {
        6 => h,
        8 => &h[..6],
        _ => return None,
    };
    u32::from_str_radix(h, 16).ok()
}

/// Relative luminance in 0..=1 (sRGB, unlinearised; good enough to pick light/dark).
pub fn luminance(c: Rgb) -> f32 {
    let r = ((c >> 16) & 0xff) as f32;
    let g = ((c >> 8) & 0xff) as f32;
    let b = (c & 0xff) as f32;
    (0.2126 * r + 0.7152 * g + 0.0722 * b) / 255.0
}

/// Linear mix of two colours; `t = 0` gives `a`, `t = 1` gives `b`.
pub fn mix(a: Rgb, b: Rgb, t: f32) -> Rgb {
    let ch = |c: Rgb, s: u32| ((c >> s) & 0xff) as f32;
    let m = |s| {
        (ch(a, s) + (ch(b, s) - ch(a, s)) * t)
            .round()
            .clamp(0.0, 255.0) as u32
    };
    (m(16) << 16) | (m(8) << 8) | m(0)
}

/// WCAG relative luminance (sRGB channels linearised).
pub fn relative_luminance(rgb: Rgb) -> f32 {
    let ch = |s: u32| {
        let v = ((rgb >> s) & 0xff) as f32 / 255.0;
        if v <= 0.040_45 {
            v / 12.92
        } else {
            ((v + 0.055) / 1.055).powf(2.4)
        }
    };
    0.2126 * ch(16) + 0.7152 * ch(8) + 0.0722 * ch(0)
}

/// WCAG contrast ratio of two colours (1 to 21).
pub fn contrast(a: Rgb, b: Rgb) -> f32 {
    let (la, lb) = (relative_luminance(a), relative_luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// `color`, pushed towards `ink` until it reaches `ratio` against `bg`
/// (icons and bars must stay visible on every theme, WCAG 1.4.11).
pub fn readable(color: Rgb, bg: Rgb, ink: Rgb, ratio: f32) -> Rgb {
    (0..=20)
        .map(|k| mix(color, ink, k as f32 / 20.0))
        .find(|c| contrast(*c, bg) >= ratio)
        .unwrap_or(ink)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hex_parsing() {
        assert_eq!(parse_hex("#a1B2c3"), Some(0xa1b2c3));
        assert_eq!(parse_hex("a1b2c3ff"), Some(0xa1b2c3));
        assert_eq!(parse_hex("#abc"), None);
        assert_eq!(parse_hex("#zzzzzz"), None);
    }

    #[test]
    fn mixing() {
        assert_eq!(mix(0x000000, 0xffffff, 0.0), 0x000000);
        assert_eq!(mix(0x000000, 0xffffff, 1.0), 0xffffff);
        assert_eq!(mix(0x000000, 0xfefefe, 0.5), 0x7f7f7f);
    }

    #[test]
    fn readable_reaches_the_ratio() {
        // Yellow on white is too faint; it darkens until it reads.
        let c = readable(0xffe066, 0xffffff, 0x1d1f24, 3.0);
        assert!(contrast(c, 0xffffff) >= 3.0);
        // Already fine: unchanged.
        assert_eq!(readable(0x2f6fed, 0xffffff, 0x000000, 3.0), 0x2f6fed);
    }
}
