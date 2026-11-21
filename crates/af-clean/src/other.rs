//! System Data — the macOS "Other" storage — mapped to named categories with
//! an honest safety level each. This module is the product's answer to
//! "how do I safely clean Other": it shows what the space actually is, and
//! only marks regenerable things as Safe.

use crate::model::Node;
use crate::model::ScanResult;
use crate::paths::home;
use crate::scan::dir_size;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Safety {
    /// Regenerable. Apps/tools rebuild it on demand. Machine-approveable.
    Safe,
    /// Real data or app state — user must look before staging.
    Review,
    /// Shown for context; never stageable from here (system-managed).
    Info,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysPath {
    pub path: PathBuf,
    pub label: String,
    pub logical: u64,
    pub physical: u64,
    pub files: u64,
    pub exists: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SysCategory {
    pub id: String,
    pub title: String,
    pub safety: Safety,
    pub hint: String,
    pub paths: Vec<SysPath>,
    pub logical: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TmSnapshot {
    pub name: String,
    /// e.g. "2026-08-30-09:12:34"
    pub date: String,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OtherReport {
    pub categories: Vec<SysCategory>,
    /// Sum of Safe categories — what "Clean safely" reclaims.
    pub safe_reclaimable: u64,
    /// Sum of Review categories — visible, staged only by explicit choice.
    pub review_reclaimable: u64,
    /// APFS local Time Machine snapshots (purged via `tmutil`, not rm).
    pub tm_snapshots: Vec<TmSnapshot>,
    /// Purgeable bytes reported by the volume, if determinable.
    pub purgeable: Option<u64>,
    pub generated_at: i64,
}

/// One category rule: well-known dirs under $HOME (macOS layout) plus a hint.
struct Rule {
    id: &'static str,
    title: &'static str,
    safety: Safety,
    hint: &'static str,
    /// $HOME-relative paths. Labels fall back to the last segment.
    paths: &'static [&'static str],
}

const RULES: &[Rule] = &[
    Rule {
        id: "user_caches",
        title: "User caches",
        safety: Safety::Safe,
        hint: "Apps rebuild these on demand. Quit an app if you want it to re-create its cache cleanly.",
        paths: &["Library/Caches"],
    },
    Rule {
        id: "diagnostics",
        title: "Crash & diagnostic reports",
        safety: Safety::Safe,
        hint: "Symbolication dumps Apple already has; nothing you need.",
        paths: &["Library/Logs/DiagnosticReports"],
    },
    Rule {
        id: "dev_caches",
        title: "Developer tool caches",
        safety: Safety::Safe,
        hint: "Package registries and toolchains re-download from the network when needed.",
        paths: &[
            ".cargo/registry",
            ".gradle/caches",
            ".m2/repository",
            ".npm",
            ".cache",
            ".nuget/packages",
            "Library/pnpm",
            "go/pkg/mod",
            "Library/Caches/ms-playwright",
        ],
    },
    Rule {
        id: "xcode_derived",
        title: "Xcode DerivedData & indexes",
        safety: Safety::Safe,
        hint: "Regenerated on the next build. Close Xcode first.",
        paths: &["Library/Developer/Xcode/DerivedData"],
    },
    Rule {
        id: "xcode_devicesupport",
        title: "iOS DeviceSupport symbols",
        safety: Safety::Safe,
        hint: "Xcode re-pulls symbols when you reconnect an iOS device of that version.",
        paths: &["Library/Developer/Xcode/iOS DeviceSupport"],
    },
    Rule {
        id: "simulators",
        title: "iOS simulators",
        safety: Safety::Review,
        hint: "Simulators re-download. Prefer `xcrun simctl delete unavailable` for the stale ones.",
        paths: &["Library/Developer/CoreSimulator/Devices", "Library/Developer/CoreSimulator/Caches"],
    },
    Rule {
        id: "build_artifacts",
        title: "Build artifacts",
        safety: Safety::Safe,
        hint: "cargo/gradle/xcodebuild output, rebuilt from source on demand. node_modules is never staged.",
        paths: &[],
    },
    Rule {
        id: "logs",
        title: "Application logs",
        safety: Safety::Review,
        hint: "Old logs are disposable; today's may matter. Keep the last few days.",
        paths: &["Library/Logs"],
    },
    Rule {
        id: "ios_backups",
        title: "iPhone / iPad backups",
        safety: Safety::Review,
        hint: "Local device backups. Delete only if iCloud Backup covers you; this is real data.",
        paths: &["Library/Application Support/MobileSync/Backup"],
    },
    Rule {
        id: "mail_downloads",
        title: "Mail attachments (downloaded)",
        safety: Safety::Review,
        hint: "Re-downloads from the mail server when you open an attachment again.",
        paths: &["Library/Containers/com.apple.mail/Data/Library/Mail Downloads"],
    },
    Rule {
        id: "docker",
        title: "Docker virtual disk",
        safety: Safety::Review,
        hint: "Reclaim from inside first: `docker system prune -a`. Deleting the disk image removes every image.",
        paths: &["Library/Containers/com.docker.docker"],
    },
    Rule {
        id: "app_support",
        title: "Application Support (per app)",
        safety: Safety::Info,
        hint: "Where abandoned apps leave their data. Shown per app so you decide; this is usually the bulk of 'Other'.",
        paths: &["Library/Application Support"],
    },
    Rule {
        id: "system_caches",
        title: "System-wide caches (/Library)",
        safety: Safety::Info,
        hint: "Needs admin to touch and macOS manages most of it. Listed for completeness.",
        paths: &["/Library/Caches"],
    },
];

/// Cache folders shared by many apps: listed per child so each can be
/// judged (and staged) on its own.
const SPLIT_DIRS: &[&str] = &["Library/Caches", ".cache"];

/// Immediate children of `dir`, sized from the scan tree when there is one.
fn children_of(scan: Option<&ScanResult>, dir: &Path) -> Vec<SysPath> {
    let mut out: Vec<SysPath> = Vec::new();
    if let Some(node) = scan.and_then(|s| find_child(&s.node, dir)) {
        for c in &node.children {
            out.push(SysPath {
                label: c.name.clone(),
                path: c.path.clone(),
                logical: c.logical,
                physical: c.physical,
                files: c.files,
                exists: true,
            });
        }
    } else {
        for k in std::fs::read_dir(dir).into_iter().flatten().flatten() {
            let (logical, physical, files) = dir_size(&k.path());
            out.push(SysPath {
                label: k.file_name().to_string_lossy().into_owned(),
                path: k.path(),
                logical,
                physical,
                files,
                exists: true,
            });
        }
    }
    out.retain(|p| p.logical > 0);
    out.sort_by(|a, b| b.logical.cmp(&a.logical));
    out
}

fn safety_rank(s: Safety) -> u8 {
    match s {
        Safety::Safe => 0,
        Safety::Review => 1,
        Safety::Info => 2,
    }
}

/// A path may match several rules (`Library/Caches/ms-playwright` is both a
/// user cache and a dev cache) or sit inside another listed path. Show it once:
/// drop a path when it, or an ancestor, is already listed under a category
/// that is at least as safe. Totals are recomputed so nothing counts twice.
pub(crate) fn dedupe_nested(categories: &mut [SysCategory]) {
    let mut all: Vec<(PathBuf, u8)> = Vec::new();
    for c in categories.iter() {
        for p in &c.paths {
            all.push((p.path.clone(), safety_rank(c.safety)));
        }
    }
    // First listing of a path wins among equals.
    let mut seen: Vec<PathBuf> = Vec::new();
    for c in categories.iter_mut() {
        let rank = safety_rank(c.safety);
        c.paths.retain(|p| {
            let covered = all
                .iter()
                .any(|(q, qrank)| q != &p.path && p.path.starts_with(q) && *qrank <= rank);
            let dup = seen.contains(&p.path);
            if !covered && !dup {
                seen.push(p.path.clone());
            }
            !covered && !dup
        });
        c.logical = c.paths.iter().map(|p| p.logical).sum();
    }
}

/// Dev-junk directory names detected from the scan tree (not fixed paths).
/// `node_modules` is deliberately absent — it stays on disk per owner policy.
const DEV_JUNK_DIRS: &[&str] = &["target", ".build", ".gradle", "Pods", "DerivedData"];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DevJunkHit {
    pub path: PathBuf,
    pub logical: u64,
}

fn probe(home: &Path, rel: &str) -> SysPath {
    let p = if rel.starts_with('/') {
        PathBuf::from(rel)
    } else {
        home.join(rel)
    };
    let exists = p.is_dir();
    let (logical, physical, files) = if exists { dir_size(&p) } else { (0, 0, 0) };
    SysPath {
        label: p
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| rel.to_string()),
        path: p,
        logical,
        physical,
        files,
        exists,
    }
}

/// Find dev-junk dirs (`target`, `.gradle`, …) in a scan tree, skipping
/// their subtrees once hit (the hit already carries the whole size).
pub fn find_dev_junk(scan: &ScanResult) -> Vec<DevJunkHit> {
    let home = home();
    let mut hits = Vec::new();
    let mut stack = vec![&scan.node];
    while let Some(n) = stack.pop() {
        let mut skip = Vec::new();
        for c in &n.children {
            // `~/.gradle` itself holds gradle.properties (signing keys, tokens):
            // only project-level build dirs count, never ones directly in Home.
            let in_home_root = n.path == home;
            if c.is_dir()
                && DEV_JUNK_DIRS.contains(&c.name.as_str())
                && c.logical > 0
                && !in_home_root
            {
                skip.push(c);
            } else if c.is_dir() {
                stack.push(c);
            }
        }
        for s in skip {
            hits.push(DevJunkHit {
                path: s.path.clone(),
                logical: s.logical,
            });
            // subtree intentionally not traversed
        }
    }
    hits.sort_by(|a, b| b.logical.cmp(&a.logical));
    hits
}

/// Build the full System Data report. `scan` (home scan, optional) supplies
/// dev-junk detection and per-app Application Support sizes; without it only
/// the fixed category paths are probed.
pub fn other_report(scan: Option<&ScanResult>, now: i64) -> OtherReport {
    let home = home();
    let mut categories = Vec::new();

    for rule in RULES {
        let mut paths = Vec::new();
        if rule.paths.is_empty() && rule.id == "build_artifacts" {
            // Dev junk comes from the scan tree when we have one.
            if let Some(s) = scan {
                let hits = find_dev_junk(s);
                let mut logical_sum = 0u64;
                for h in hits.into_iter().take(200) {
                    logical_sum += h.logical;
                    paths.push(SysPath {
                        label: "build cache".into(),
                        path: h.path,
                        logical: h.logical,
                        physical: 0,
                        files: 0,
                        exists: true,
                    });
                }
                categories.push(SysCategory {
                    id: rule.id.to_string(),
                    title: rule.title.to_string(),
                    safety: rule.safety,
                    hint: rule.hint.to_string(),
                    paths,
                    logical: logical_sum,
                });
            }
            continue;
        }

        let mut sum = 0u64;
        for rel in rule.paths {
            let sp = probe(&home, rel);
            if !sp.exists {
                continue;
            }
            sum += sp.logical;
            if SPLIT_DIRS.contains(rel) {
                // One row per app or tool instead of a single opaque total.
                paths.extend(children_of(scan, &sp.path));
            } else {
                paths.push(sp);
            }
        }
        if rule.id == "app_support" && !paths.is_empty() {
            // Break the monolith down per app: one SysPath per child dir.
            let base = paths.remove(0);
            let mut per_app: Vec<SysPath> = Vec::new();
            if let Some(s) = scan {
                if let Some(node) = find_child(&s.node, &base.path) {
                    for c in node.children.iter().take(300) {
                        per_app.push(SysPath {
                            label: c.name.clone(),
                            path: c.path.clone(),
                            logical: c.logical,
                            physical: c.physical,
                            files: c.files,
                            exists: true,
                        });
                    }
                    per_app.sort_by(|a, b| b.logical.cmp(&a.logical));
                    sum = base.logical;
                }
            } else {
                // No scan: top-level children only, sized on demand.
                let kids = std::fs::read_dir(&base.path)
                    .into_iter()
                    .flatten()
                    .flatten();
                for k in kids.take(300) {
                    let (l, p, f) = dir_size(&k.path());
                    sum += l;
                    per_app.push(SysPath {
                        label: k.file_name().to_string_lossy().into_owned(),
                        path: k.path(),
                        logical: l,
                        physical: p,
                        files: f,
                        exists: true,
                    });
                }
                per_app.sort_by(|a, b| b.logical.cmp(&a.logical));
            }
            categories.push(SysCategory {
                id: rule.id.to_string(),
                title: rule.title.to_string(),
                safety: rule.safety,
                hint: rule.hint.to_string(),
                paths: per_app,
                logical: sum,
            });
            continue;
        }

        if !paths.is_empty() {
            categories.push(SysCategory {
                id: rule.id.to_string(),
                title: rule.title.to_string(),
                safety: rule.safety,
                hint: rule.hint.to_string(),
                paths,
                logical: sum,
            });
        }
    }

    dedupe_nested(&mut categories);
    categories.sort_by(|a, b| b.logical.cmp(&a.logical));
    let safe_reclaimable = categories
        .iter()
        .filter(|c| c.safety == Safety::Safe)
        .map(|c| c.logical)
        .sum();
    let review_reclaimable = categories
        .iter()
        .filter(|c| c.safety == Safety::Review)
        .map(|c| c.logical)
        .sum();

    let (tm_snapshots, purgeable) = crate::sysdata::tm_and_purgeable();

    OtherReport {
        categories,
        safe_reclaimable,
        review_reclaimable,
        tm_snapshots,
        purgeable,
        generated_at: now,
    }
}

fn find_child<'a>(root: &'a Node, path: &Path) -> Option<&'a Node> {
    find_child_rec(root, path)
}

fn find_child_rec<'a>(node: &'a Node, path: &Path) -> Option<&'a Node> {
    if node.path == path {
        return Some(node);
    }
    // Prune: only descend if the target could be under this dir.
    if !path.starts_with(&node.path) {
        return None;
    }
    node.children.iter().find_map(|c| find_child_rec(c, path))
}

#[cfg(test)]
mod dedupe_tests {
    use super::*;

    fn sp(p: &str, logical: u64) -> SysPath {
        SysPath {
            label: p.into(),
            path: PathBuf::from(p),
            logical,
            physical: 0,
            files: 0,
            exists: true,
        }
    }
    fn cat(id: &str, safety: Safety, paths: Vec<SysPath>) -> SysCategory {
        let logical = paths.iter().map(|p| p.logical).sum();
        SysCategory {
            id: id.into(),
            title: id.into(),
            safety,
            hint: String::new(),
            paths,
            logical,
        }
    }

    #[test]
    fn nested_and_repeated_paths_count_once() {
        let mut cats = vec![
            cat(
                "user_caches",
                Safety::Safe,
                vec![
                    sp("/h/Library/Caches/ms-playwright", 5),
                    sp("/h/Library/Caches/app", 3),
                ],
            ),
            cat(
                "dev_caches",
                Safety::Safe,
                vec![sp("/h/Library/Caches/ms-playwright", 5), sp("/h/.npm", 2)],
            ),
            cat(
                "build",
                Safety::Safe,
                vec![sp("/h/p/target", 10), sp("/h/p/target/debug", 9)],
            ),
        ];
        dedupe_nested(&mut cats);
        let total: u64 = cats.iter().map(|c| c.logical).sum();
        assert_eq!(total, 5 + 3 + 2 + 10);
        assert_eq!(cats[1].paths.len(), 1);
    }

    #[test]
    fn a_review_ancestor_does_not_hide_a_safe_child() {
        let mut cats = vec![
            cat("logs", Safety::Review, vec![sp("/h/Library/Logs", 10)]),
            cat(
                "diag",
                Safety::Safe,
                vec![sp("/h/Library/Logs/DiagnosticReports", 4)],
            ),
        ];
        dedupe_nested(&mut cats);
        assert_eq!(cats[1].paths.len(), 1);
    }
}
