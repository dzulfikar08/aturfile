//! Preferences, kept as TOML in the app's data folder (the container in the
//! App Sandbox).

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// `system` or a bundled theme's name.
    pub theme: String,
    /// `compact` or `airy`.
    pub density: String,
    /// `list`, `columns` or `icons`.
    pub view: String,
    pub show_hidden: bool,
    pub show_sidebar: bool,
    pub show_preview: bool,
    pub sidebar_width: f32,
    pub preview_width: f32,
    /// List view column widths: Modified, Size, Kind.
    pub columns: [f32; 3],
    /// Folders pinned to the sidebar's Favorites.
    pub pins: Vec<PathBuf>,
    /// Cmd-K searches file names as well as commands (Tab switches).
    pub palette_files: bool,
    /// Check for updates once a day (Developer ID builds; the App Store
    /// updates its own).
    pub auto_update: bool,
}

impl Default for Settings {
    fn default() -> Self {
        use af_ui::metrics::*;
        Self {
            theme: "system".into(),
            density: "compact".into(),
            view: "list".into(),
            show_hidden: false,
            show_sidebar: true,
            show_preview: false,
            sidebar_width: SIDEBAR_WIDTH,
            preview_width: PREVIEW_WIDTH,
            columns: [COL_MODIFIED, COL_SIZE, COL_KIND],
            pins: Vec::new(),
            palette_files: true,
            auto_update: true,
        }
    }
}

impl Settings {
    pub fn file() -> PathBuf {
        af_mac::app_data_dir().join("settings.toml")
    }

    /// The saved settings, or the defaults when there are none (or they
    /// don't parse: a broken file never stops the app from starting).
    pub fn load() -> Self {
        Self::load_from(&Self::file())
    }

    pub fn load_from(file: &Path) -> Self {
        std::fs::read_to_string(file)
            .ok()
            .and_then(|s| toml::from_str(&s).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        self.save_to(&Self::file())
    }

    pub fn save_to(&self, file: &Path) -> std::io::Result<()> {
        let text = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        if let Some(dir) = file.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = file.with_extension("toml.tmp");
        std::fs::write(&tmp, text)?;
        std::fs::rename(tmp, file)
    }

    pub fn is_pinned(&self, path: &Path) -> bool {
        self.pins.iter().any(|p| p == path)
    }

    /// Pin or unpin `path`; returns whether it is pinned now.
    pub fn toggle_pin(&mut self, path: &Path) -> bool {
        if self.is_pinned(path) {
            self.pins.retain(|p| p != path);
            false
        } else {
            self.pins.push(path.to_path_buf());
            true
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_and_tolerates_junk() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("sub/settings.toml");
        let mut s = Settings {
            theme: "Nord".into(),
            ..Settings::default()
        };
        assert!(s.toggle_pin(Path::new("/tmp")));
        s.save_to(&file).unwrap();
        assert_eq!(Settings::load_from(&file), s);
        assert!(!s.toggle_pin(Path::new("/tmp")));
        std::fs::write(&file, "theme = [not toml").unwrap();
        assert_eq!(Settings::load_from(&file), Settings::default());
        // Old files missing new keys still load.
        std::fs::write(&file, "density = \"airy\"\n").unwrap();
        assert_eq!(Settings::load_from(&file).density, "airy");
    }
}
