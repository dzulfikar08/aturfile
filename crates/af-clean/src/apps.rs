//! App inventory and the complete-uninstall mapping: a .app bundle plus the
//! files it scattered into ~/Library (prefs, caches, containers, web data…).

use crate::model::Node;
use crate::paths::home;
use crate::scan::dir_size;
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledApp {
    pub name: String,
    pub bundle_id: String,
    pub version: String,
    pub path: PathBuf,
    /// Bundle size on disk (physical).
    pub bundle_logical: u64,
    pub bundle_physical: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Leftover {
    pub path: PathBuf,
    pub logical: u64,
    /// Which Library area it came from.
    pub area: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallPlan {
    pub app: InstalledApp,
    pub leftovers: Vec<Leftover>,
    pub total_logical: u64,
}

/// Enumerate .app bundles in /Applications and ~/Applications.
pub fn list_apps() -> Vec<InstalledApp> {
    let home = home();
    let mut roots = vec![PathBuf::from("/Applications"), home.join("Applications")];
    roots.retain(|r| r.is_dir());
    let mut apps = Vec::new();
    for root in roots {
        let Ok(entries) = fs::read_dir(&root) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            if p.extension()
                .map(|e| e.eq_ignore_ascii_case("app"))
                .unwrap_or(false)
            {
                if let Some(app) = read_app(&p) {
                    apps.push(app);
                }
            }
        }
    }
    apps.sort_by_key(|x| std::cmp::Reverse(x.bundle_logical));
    apps
}

fn read_app(bundle: &Path) -> Option<InstalledApp> {
    let plist_path = bundle.join("Contents/Info.plist");
    let value = plist::Value::from_file(&plist_path).ok()?;
    let dict = value.as_dictionary()?;
    let name = dict
        .get("CFBundleName")
        .and_then(|v| v.as_string())
        .map(String::from)
        .unwrap_or_else(|| {
            bundle
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
                .unwrap_or_default()
        });
    let (bundle_logical, bundle_physical, _) = dir_size(bundle);
    Some(InstalledApp {
        name,
        bundle_id: dict
            .get("CFBundleIdentifier")
            .and_then(|v| v.as_string())
            .unwrap_or_default()
            .to_string(),
        version: dict
            .get("CFBundleShortVersionString")
            .and_then(|v| v.as_string())
            .unwrap_or_default()
            .to_string(),
        path: bundle.to_path_buf(),
        bundle_logical,
        bundle_physical,
    })
}

/// Every ~/Library area where an app leaves traces, with the naming scheme it
/// uses there.
const AREAS: &[(&str, Match)] = &[
    ("Application Support", Match::NameOrId),
    ("Caches", Match::NameOrId),
    ("Preferences", Match::IdPlist),
    ("Logs", Match::NameOrId),
    ("Containers", Match::Id),
    ("Group Containers", Match::GroupPrefix),
    ("WebKit", Match::NameOrId),
    ("Saved Application State", Match::IdState),
    ("HTTPStorages", Match::IdOrCookies),
    ("Cookies", Match::IdCookies),
];

#[derive(Clone, Copy)]
enum Match {
    /// dir named after app name (case/space-insensitive) or bundle id
    NameOrId,
    /// `<bundle-id>.plist`
    IdPlist,
    /// dir exactly `<bundle-id>`
    Id,
    /// `group.<bundle-id>*`
    GroupPrefix,
    /// `<bundle-id>.savedState`
    IdState,
    /// `<bundle-id>` or `<bundle-id>.binarycookies`
    IdOrCookies,
    /// `<bundle-id>.binarycookies`
    IdCookies,
}

/// Normalize for fuzzy name match: lowercase, strip spaces/dashes/underscores.
fn norm(s: &str) -> String {
    s.to_lowercase()
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect()
}

fn matches(stem: &str, app: &InstalledApp, m: Match) -> bool {
    let stem_norm = norm(stem);
    let id = app.bundle_id.as_str();
    if id.is_empty() {
        return false;
    }
    match m {
        Match::Id | Match::IdPlist | Match::IdState | Match::IdCookies => stem == id,
        Match::IdOrCookies => stem == id || stem == format!("{id}.binarycookies"),
        Match::GroupPrefix => {
            stem == format!("group.{id}") || stem.starts_with(&format!("group.{id}."))
        }
        Match::NameOrId => stem == id || (!app.name.is_empty() && stem_norm == norm(&app.name)),
    }
}

/// Find everything `app` left behind in ~/Library (and the bundle itself is
/// the caller's first entry when building a plan).
pub fn find_leftovers(app: &InstalledApp) -> Vec<Leftover> {
    let lib = home().join("Library");
    let mut out = Vec::new();
    for (area, m) in AREAS {
        let dir = lib.join(area);
        let Ok(entries) = fs::read_dir(&dir) else {
            continue;
        };
        for e in entries.flatten() {
            let p = e.path();
            let stem = p
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let file_stem = p
                .file_stem()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            let hit = match m {
                Match::IdPlist => file_stem == app.bundle_id,
                _ => matches(&stem, app, *m),
            };
            if hit {
                let (logical, _, _) = dir_size(&p);
                if logical > 0 {
                    out.push(Leftover {
                        path: p,
                        logical,
                        area: area.to_string(),
                    });
                }
            }
        }
    }
    out.sort_by_key(|x| std::cmp::Reverse(x.logical));
    out
}

/// Bundle + leftovers, ready to hand to the cleanup queue.
pub fn uninstall_plan(app: &InstalledApp) -> UninstallPlan {
    let leftovers = find_leftovers(app);
    let total_logical = app.bundle_logical + leftovers.iter().map(|l| l.logical).sum::<u64>();
    UninstallPlan {
        app: app.clone(),
        leftovers,
        total_logical,
    }
}

/// Aggregate app-support usage per app from a home scan (used by the UI's
/// Application Support drill-down).
pub fn app_support_from_scan(root: &Node, home: &Path) -> Vec<(String, u64)> {
    let target = home.join("Library/Application Support");
    fn rec(node: &Node, target: &Path, out: &mut Vec<(String, u64)>) {
        if node.path == target {
            for c in &node.children {
                if c.is_dir() {
                    out.push((c.name.clone(), c.logical));
                }
            }
            return;
        }
        if target.starts_with(&node.path) {
            for c in &node.children {
                rec(c, target, out);
            }
        }
    }
    let mut out = Vec::new();
    rec(root, &target, &mut out);
    out.sort_by_key(|x| std::cmp::Reverse(x.1));
    out
}
