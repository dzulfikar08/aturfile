//! macOS system-level disk facts: APFS local Time Machine snapshots and
//! volume purgeable space, via `tmutil` / `diskutil`. Parsing is pure and
//! unit-tested here; the commands themselves only run on macOS.

use crate::other::TmSnapshot;

/// Parse `tmutil listlocalsnapshots /` output. Pure — testable everywhere.
/// Lines look like: `com.apple.TimeMachine.2026-08-30-091234.local`
pub fn parse_tmutil(output: &str) -> Vec<TmSnapshot> {
    output
        .lines()
        .filter_map(|l| {
            let l = l.trim();
            let stem = l.strip_prefix("com.apple.TimeMachine.")?;
            let date = stem.strip_suffix(".local")?;
            if date.len() != 17 || date.as_bytes().get(10) != Some(&b'-') {
                return None;
            }
            let mut d = date.to_string();
            d.replace_range(10..11, " "); // 2026-08-30-091234 -> 2026-08-30 091234
            d.insert(13, ':');
            d.insert(16, ':');
            Some(TmSnapshot {
                name: l.to_string(),
                date: d,
            })
        })
        .collect()
}

#[cfg(target_os = "macos")]
pub fn list_tm_snapshots() -> Vec<TmSnapshot> {
    std::process::Command::new("tmutil")
        .args(["listlocalsnapshots", "/"])
        .output()
        .map(|o| parse_tmutil(&String::from_utf8_lossy(&o.stdout)))
        .unwrap_or_default()
}

#[cfg(not(target_os = "macos"))]
pub fn list_tm_snapshots() -> Vec<TmSnapshot> {
    Vec::new()
}

/// Best-effort purgeable bytes for `/` from `diskutil info -plist`.
#[cfg(target_os = "macos")]
fn purgeable_bytes() -> Option<u64> {
    let out = std::process::Command::new("diskutil")
        .args(["info", "-plist", "/"])
        .output()
        .ok()?;
    let v = plist::Value::from_reader(std::io::Cursor::new(out.stdout)).ok()?;
    let dict = v.as_dictionary()?;
    dict.iter()
        .find(|(k, _)| k.to_lowercase().contains("purgeable"))
        .and_then(|(_, v)| v.as_real().map(|f| f as u64))
}

#[cfg(target_os = "macos")]
/// Delete one local snapshot by its full name
/// (`com.apple.TimeMachine.YYYY-MM-DD-HHMMSS.local`).
pub fn delete_tm_snapshot(name: &str) -> Result<(), String> {
    let out = std::process::Command::new("tmutil")
        .args(["deletelocalsnapshots", name])
        .output()
        .map_err(|e| e.to_string())?;
    if out.status.success() {
        Ok(())
    } else {
        Err(String::from_utf8_lossy(&out.stderr).into_owned())
    }
}

/// Snapshots + purgeable for the Other report. Safe on any OS.
pub fn tm_and_purgeable() -> (Vec<TmSnapshot>, Option<u64>) {
    let snaps = list_tm_snapshots();
    let purgeable = {
        #[cfg(target_os = "macos")]
        {
            purgeable_bytes()
        }
        #[cfg(not(target_os = "macos"))]
        {
            None
        }
    };
    (snaps, purgeable)
}

#[cfg(not(target_os = "macos"))]
/// Stub so the desktop shell compiles off-macOS; the action is macOS-only.
pub fn delete_tm_snapshot(_name: &str) -> Result<(), String> {
    Err("tmutil is macOS-only".into())
}
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tmutil_lines() {
        let out = "Snapshot dates for /:\ncom.apple.TimeMachine.2026-08-30-091234.local\ncom.apple.TimeMachine.2026-09-01-101500.local\n";
        let snaps = parse_tmutil(out);
        assert_eq!(snaps.len(), 2);
        assert_eq!(
            snaps[0].name,
            "com.apple.TimeMachine.2026-08-30-091234.local"
        );
        assert_eq!(snaps[0].date, "2026-08-30 09:12:34");
        assert_eq!(snaps[1].date, "2026-09-01 10:15:00");
    }

    #[test]
    fn ignores_garbage() {
        assert!(parse_tmutil("No snapshots.\nhello\ncom.apple.TimeMachine.bad\n").is_empty());
    }
}
