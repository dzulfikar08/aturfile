//! Scan snapshots: persist a scan to JSON and diff two of them to see which
//! folders grew. (Disk-side APFS snapshots live in [`crate::sysdata`].)

use crate::model::{Kind, Node, ScanResult};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathDelta {
    pub path: PathBuf,
    pub old_bytes: u64,
    pub new_bytes: u64,
    pub delta: i64,
}

pub fn save(scan: &ScanResult, file: &Path) -> crate::DbResult<()> {
    if let Some(parent) = file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(scan)?;
    std::fs::write(file, json)?;
    Ok(())
}

pub fn load(file: &Path) -> crate::DbResult<ScanResult> {
    let json = std::fs::read_to_string(file)?;
    Ok(serde_json::from_str(&json)?)
}

/// Compare two scans of (ideally) the same root. Returns changed paths sorted
/// by growth, biggest gainer first. New items have `old_bytes = 0`; removed
/// items appear with `new_bytes = 0` and negative delta.
pub fn diff(old: &ScanResult, new: &ScanResult) -> Vec<PathDelta> {
    let mut index: std::collections::HashMap<PathBuf, (u64, bool)> = Default::default();
    old.node.walk(&mut |n| {
        index.insert(n.path.clone(), (n.logical, n.is_dir()));
    });
    let mut out = Vec::new();
    new.node.walk(&mut |n| {
        // The scan root is always "changed"; it is not a useful diff row.
        if n.path == new.root {
            return;
        }
        let cur = (n.logical, n.is_dir());
        match index.remove(&n.path) {
            Some(prev) if prev == cur => {}
            Some(prev) => out.push(PathDelta {
                path: n.path.clone(),
                old_bytes: prev.0,
                new_bytes: n.logical,
                delta: n.logical as i64 - prev.0 as i64,
            }),
            None => out.push(PathDelta {
                path: n.path.clone(),
                old_bytes: 0,
                new_bytes: n.logical,
                delta: n.logical as i64,
            }),
        }
    });
    // Anything left in `index` existed only in the old scan → removed.
    for (path, (bytes, _)) in index {
        if path == old.root {
            continue; // scan root is excluded from diffs
        }
        out.push(PathDelta {
            path,
            old_bytes: bytes,
            new_bytes: 0,
            delta: -(bytes as i64),
        });
    }
    out.retain(|d| d.delta != 0);
    out.sort_by_key(|x| std::cmp::Reverse(x.delta));
    out
}

/// Deep-clone a tree pruning files smaller than `min` into one synthetic
/// `· files` leaf per parent, so exports stay small without lying about
/// sizes. Directories deeper than `max_depth` collapse into leaves (their
/// aggregated sizes already include everything below them).
pub fn prune_for_export(node: &Node, min_file_bytes: u64, max_depth: usize) -> Node {
    let mut out = Node {
        name: node.name.clone(),
        path: node.path.clone(),
        kind: node.kind,
        logical: node.logical,
        physical: node.physical,
        files: node.files,
        dirs: node.dirs,
        mtime: node.mtime,
        children: Vec::new(),
    };
    if max_depth == 0 {
        return out;
    }
    let mut small_logical = 0u64;
    let mut small_count = 0u64;
    for c in &node.children {
        if c.is_dir() {
            out.children
                .push(prune_for_export(c, min_file_bytes, max_depth - 1));
        } else if c.logical >= min_file_bytes {
            out.children.push(c.clone());
        } else {
            small_logical += c.logical;
            small_count += 1;
        }
    }
    if small_count > 0 {
        let name = format!("· {small_count} smaller files");
        out.children.push(Node {
            name: name.clone(),
            path: node.path.join(&name),
            kind: Kind::File,
            logical: small_logical,
            physical: small_logical,
            files: small_count,
            dirs: 0,
            mtime: node.mtime,
            children: Vec::new(),
        });
    }
    out
}
