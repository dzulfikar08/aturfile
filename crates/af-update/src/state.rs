//! What the updater remembers between runs (`update.json` in the app's data
//! folder): when it last checked, and a staged update waiting for a restart.

use std::path::Path;

use serde::{Deserialize, Serialize};

use crate::install::Staged;

/// Automatic checks run at most this often.
pub const CHECK_INTERVAL_SECS: u64 = 24 * 60 * 60;

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct State {
    /// Unix seconds of the last automatic or manual check.
    pub last_check: Option<u64>,
    pub staged: Option<Staged>,
}

/// An automatic check is due: never checked, a day has passed, or the clock
/// went backwards (so a wrong clock can't stop checks for good).
pub fn due(last_check: Option<u64>, now: u64) -> bool {
    match last_check {
        None => true,
        Some(last) => now < last || now - last >= CHECK_INTERVAL_SECS,
    }
}

pub fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

impl State {
    /// The saved state, or the default when there is none or it doesn't parse.
    pub fn load_from(path: &Path) -> Self {
        std::fs::read(path)
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    /// Write atomically (a temp file, then a rename).
    pub fn save_to(&self, path: &Path) -> std::io::Result<()> {
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(&tmp, path)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn throttle() {
        let day = CHECK_INTERVAL_SECS;
        assert!(due(None, 1_000));
        assert!(!due(Some(1_000), 1_000));
        assert!(!due(Some(1_000), 1_000 + day - 1));
        assert!(due(Some(1_000), 1_000 + day));
        assert!(due(Some(1_000 + day * 10), 1_000), "clock went backwards");
    }

    #[test]
    fn round_trip() {
        let d = tempfile::tempdir().unwrap();
        let path = d.path().join("s/update.json");
        assert_eq!(State::load_from(&path), State::default());
        let s = State {
            last_check: Some(42),
            staged: Some(Staged {
                version: "1.1.0".into(),
                notes_url: "https://x".into(),
                bundle: "/Applications/aturfile.app".into(),
                new_app: "/Applications/.aturfile-update/x/aturfile.app".into(),
                work_dir: Some("/Applications/.aturfile-update".into()),
            }),
        };
        s.save_to(&path).unwrap();
        assert_eq!(State::load_from(&path), s);
        std::fs::write(&path, "{ nope").unwrap();
        assert_eq!(State::load_from(&path), State::default());
    }
}
