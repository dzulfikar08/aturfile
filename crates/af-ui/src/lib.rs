//! aturfile's design system, ported from Kantor (same author; see NOTICE.md).
//!
//! The rule: **views never use raw colours or sizes**. Sizes come from
//! [`metrics`], colours from [`tokens`], which are derived from a [`Palette`]
//! and also mapped onto GPUI Kit's theme so its components match.

pub mod assets;
pub mod catalog;
pub mod components;
pub mod fonts;
pub mod keys;
pub mod metrics;
pub mod palette;
pub mod tokens;

pub use assets::AppAssets;
pub use palette::{Palette, Rgb, mix};
pub use tokens::{Tokens, tokens};
