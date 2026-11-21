//! One dark palette for now; a light one comes with the settings window.

use gpui::{Hsla, rgb};

pub fn bg() -> Hsla {
    rgb(0x16181c).into()
}
pub fn sidebar() -> Hsla {
    rgb(0x1d2025).into()
}
pub fn panel() -> Hsla {
    rgb(0x22262c).into()
}
pub fn border() -> Hsla {
    rgb(0x2e333a).into()
}
pub fn text() -> Hsla {
    rgb(0xe6e8eb).into()
}
pub fn muted() -> Hsla {
    rgb(0x8b929c).into()
}
pub fn accent() -> Hsla {
    rgb(0x3d7eff).into()
}
pub fn selected() -> Hsla {
    rgb(0x2a4a86).into()
}
pub fn hover() -> Hsla {
    rgb(0x262a31).into()
}
pub fn folder() -> Hsla {
    rgb(0x6aa5ff).into()
}
pub fn safe() -> Hsla {
    rgb(0x3fb97a).into()
}
pub fn review() -> Hsla {
    rgb(0xe0a23a).into()
}
pub fn danger() -> Hsla {
    rgb(0xe5534b).into()
}
