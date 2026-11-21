//! Read-only views over a scan tree: Age Map buckets, Big & Untouched,
//! month heatmap, Top Sizes ranking. Pure tree walks — no filesystem access.

use crate::model::Node;
use crate::scan::ScanResult;
use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AgeBuckets {
    pub last_7d: u64,
    pub last_30d: u64,
    pub last_90d: u64,
    pub last_year: u64,
    pub older: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BigUntouchedItem {
    pub path: std::path::PathBuf,
    pub logical: u64,
    /// Days since last modification.
    pub age_days: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonthCell {
    /// e.g. "2026-08"
    pub month: String,
    pub bytes: u64,
}

/// Bytes by how long since their last modification.
pub fn age_buckets(scan: &ScanResult, now: i64) -> AgeBuckets {
    let mut b = AgeBuckets {
        last_7d: 0,
        last_30d: 0,
        last_90d: 0,
        last_year: 0,
        older: 0,
    };
    scan.node.walk(&mut |n| {
        if n.is_dir() {
            return;
        }
        let age = (now - n.mtime).max(0);
        if age < 7 * 86_400 {
            b.last_7d += n.logical;
        } else if age < 30 * 86_400 {
            b.last_30d += n.logical;
        } else if age < 90 * 86_400 {
            b.last_90d += n.logical;
        } else if age < 365 * 86_400 {
            b.last_year += n.logical;
        } else {
            b.older += n.logical;
        }
    });
    b
}

/// Large files untouched for `older_than_days` — the "Big & Untouched" list.
pub fn big_untouched(
    scan: &ScanResult,
    min_bytes: u64,
    older_than_days: u64,
    now: i64,
) -> Vec<BigUntouchedItem> {
    let cutoff = older_than_days * 86_400;
    let mut out = Vec::new();
    scan.node.walk(&mut |n| {
        if n.is_dir() || n.logical < min_bytes {
            return;
        }
        let age = (now - n.mtime).max(0) as u64;
        if age >= cutoff {
            out.push(BigUntouchedItem {
                path: n.path.clone(),
                logical: n.logical,
                age_days: age / 86_400,
            });
        }
    });
    out.sort_by(|a, b| b.logical.cmp(&a.logical));
    out
}

/// Bytes grouped by (year, month) of mtime, chronological. Feeds the Age Map
/// heatmap.
pub fn month_heatmap(scan: &ScanResult) -> Vec<MonthCell> {
    let mut map: std::collections::BTreeMap<String, u64> = Default::default();
    scan.node.walk(&mut |n| {
        if n.is_dir() {
            return;
        }
        let dt: DateTime<Utc> = DateTime::from_timestamp(n.mtime, 0).unwrap_or_default();
        *map.entry(dt.format("%Y-%m").to_string()).or_default() += n.logical;
    });
    map.into_iter()
        .map(|(month, bytes)| MonthCell { month, bytes })
        .collect()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopItem {
    pub path: std::path::PathBuf,
    pub name: String,
    pub kind: crate::model::Kind,
    pub logical: u64,
    pub physical: u64,
    pub files: u64,
    /// Share of the scan root, 0.0–1.0.
    pub share: f64,
}

/// The N biggest things in the scan, files and folders together.
pub fn top_sizes(scan: &ScanResult, n: usize) -> Vec<TopItem> {
    let total = scan.node.logical.max(1);
    let mut all: Vec<&Node> = Vec::new();
    scan.node.walk(&mut |node| {
        if node.path != scan.node.path {
            all.push(node);
        }
    });
    all.sort_by(|a, b| b.logical.cmp(&a.logical));
    all.into_iter()
        .take(n)
        .map(|node| TopItem {
            path: node.path.clone(),
            name: node.name.clone(),
            kind: node.kind,
            logical: node.logical,
            physical: node.physical,
            files: node.files,
            share: node.logical as f64 / total as f64,
        })
        .collect()
}

/// APFS compression saving across the whole scan: logical minus physical.
pub fn compression_saved(scan: &ScanResult) -> u64 {
    scan.node.logical.saturating_sub(scan.node.physical)
}
