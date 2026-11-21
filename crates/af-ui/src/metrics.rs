//! Sizes (Kantor's scale, from Voltrus Tools' `vt-ui` metrics).

use gpui_kit::{Pixels, px};

// Type scale
pub const FONT_CAPTION: Pixels = px(10.5);
pub const FONT_SMALL: Pixels = px(11.5);
pub const FONT_BODY: Pixels = px(12.5);
pub const FONT_EMPHASIS: Pixels = px(13.);
pub const FONT_TITLE: Pixels = px(15.);
pub const FONT_DISPLAY: Pixels = px(22.);

/// Root rem size; GPUI Kit components scale from it (14 = compact, desktop-like).
pub const REM: Pixels = px(14.);

// Spacing scale
pub const SPACE_1: Pixels = px(2.);
pub const SPACE_2: Pixels = px(4.);
pub const SPACE_3: Pixels = px(6.);
pub const SPACE_4: Pixels = px(8.);
pub const SPACE_5: Pixels = px(10.);
pub const SPACE_6: Pixels = px(12.);
pub const SPACE_7: Pixels = px(16.);
pub const SPACE_8: Pixels = px(20.);
pub const SPACE_9: Pixels = px(24.);
pub const SPACE_10: Pixels = px(32.);

// Radii
pub const RADIUS_SM: Pixels = px(4.);
pub const RADIUS_MD: Pixels = px(6.);
pub const RADIUS_LG: Pixels = px(8.);
pub const RADIUS_XL: Pixels = px(12.);

// Controls and icons
pub const CONTROL_SM: Pixels = px(22.);
pub const CONTROL_MD: Pixels = px(26.);
pub const CONTROL_LG: Pixels = px(32.);
pub const ICON_SM: Pixels = px(12.);
pub const ICON_MD: Pixels = px(14.);
pub const ICON_LG: Pixels = px(16.);

// Shell
pub const TITLE_BAR_HEIGHT: Pixels = px(38.);
/// Room for the macOS traffic lights at the title bar's left edge.
pub const TRAFFIC_LIGHTS_WIDTH: Pixels = px(78.);
pub const TOOLBAR_HEIGHT: Pixels = px(40.);
pub const STATUS_BAR_HEIGHT: Pixels = px(26.);
pub const TAB_HEIGHT: Pixels = px(28.);
/// Text only truncates inside a known width, so tab titles get a fixed one.
pub const TAB_TITLE_WIDTH: Pixels = px(132.);
pub const HEADER_HEIGHT: Pixels = px(26.);
pub const PALETTE_WIDTH: Pixels = px(560.);
pub const PALETTE_TOP: Pixels = px(88.);
pub const PALETTE_ROW_HEIGHT: Pixels = px(32.);
pub const PALETTE_FOOTER_HEIGHT: Pixels = px(32.);
pub const PALETTE_MAX_ROWS: usize = 10;

// Panels
pub const SIDEBAR_WIDTH: f32 = 208.;
pub const SIDEBAR_MIN: f32 = 150.;
pub const SIDEBAR_MAX: f32 = 360.;
pub const PREVIEW_WIDTH: f32 = 300.;
pub const PREVIEW_MIN: f32 = 220.;
pub const PREVIEW_MAX: f32 = 640.;
/// Width of the invisible drag strip on a panel edge.
pub const RESIZE_HANDLE: Pixels = px(6.);

// File views
/// Miller column width.
pub const COLUMN_WIDTH: f32 = 230.;
/// Icon view cell.
pub const ICON_CELL_WIDTH: f32 = 112.;
pub const ICON_CELL_HEIGHT: f32 = 104.;
pub const ICON_TILE: Pixels = px(56.);
/// List columns: default and minimum widths.
pub const COL_MODIFIED: f32 = 150.;
pub const COL_SIZE: f32 = 86.;
pub const COL_KIND: f32 = 120.;
pub const COL_MIN: f32 = 56.;
pub const COL_MAX: f32 = 400.;

/// Row spacing in the file views.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Density {
    #[default]
    Compact,
    Airy,
}

impl Density {
    pub fn row_height(self) -> Pixels {
        match self {
            Self::Compact => px(26.),
            Self::Airy => px(32.),
        }
    }

    pub fn sidebar_row(self) -> Pixels {
        match self {
            Self::Compact => px(26.),
            Self::Airy => px(30.),
        }
    }

    pub fn parse(s: &str) -> Self {
        if s.eq_ignore_ascii_case("airy") {
            Self::Airy
        } else {
            Self::Compact
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::Compact => "compact",
            Self::Airy => "airy",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Compact => "Compact",
            Self::Airy => "Airy",
        }
    }
}
