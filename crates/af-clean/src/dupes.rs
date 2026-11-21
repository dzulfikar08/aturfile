//! Content-hash duplicate finder: size prefilter → head-of-file hash → full
//! XXH3-128 over 64 KiB chunks. Byte-for-byte equality, not name equality.

use crate::model::Node;
use crate::scan::ScanResult;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::Read;
use std::path::PathBuf;
use xxhash_rust::xxh3::{xxh3_128, Xxh3};

const CHUNK: usize = 64 * 1024;
const HEAD_BYTES: usize = 4 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DupFile {
    pub path: PathBuf,
    pub logical: u64,
    pub mtime: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DuplicateGroup {
    pub files: Vec<DupFile>,
    /// Size of one copy.
    pub size: u64,
    /// size × (copies − 1): what you get back keeping one.
    pub wasted: u64,
    /// Suggested keeper: newest mtime wins (ties → first path).
    pub keep: PathBuf,
}

/// Collect candidate files from a scan tree (files only, above `min_size`).
fn candidates(root: &Node, min_size: u64) -> Vec<&Node> {
    let mut out = Vec::new();
    let mut stack = vec![root];
    while let Some(n) = stack.pop() {
        for c in &n.children {
            if c.is_dir() {
                stack.push(c);
            } else if c.logical >= min_size {
                out.push(c);
            }
        }
    }
    out
}

fn hash_file(path: &std::path::Path, full: bool) -> std::io::Result<u128> {
    let mut f = File::open(path)?;
    if !full {
        let mut head = [0u8; HEAD_BYTES];
        let n = read_up_to(&mut f, &mut head)?;
        return Ok(xxh3_128(&head[..n]));
    }
    let mut h = Xxh3::new();
    let mut buf = vec![0u8; CHUNK];
    loop {
        let n = read_up_to(&mut f, &mut buf)?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(h.digest128())
}

fn read_up_to(f: &mut File, buf: &mut [u8]) -> std::io::Result<usize> {
    let mut filled = 0;
    while filled < buf.len() {
        let n = f.read(&mut buf[filled..])?;
        if n == 0 {
            break;
        }
        filled += n;
    }
    Ok(filled)
}

/// Find duplicate groups across the scanned tree.
pub fn find_duplicates(scan: &ScanResult, min_size: u64) -> std::io::Result<Vec<DuplicateGroup>> {
    // 1. size bucket
    let mut by_size: HashMap<u64, Vec<&Node>> = HashMap::new();
    for n in candidates(&scan.node, min_size) {
        by_size.entry(n.logical).or_default().push(n);
    }
    by_size.retain(|_, v| v.len() > 1);

    // 2. head hash bucket
    let mut by_head: HashMap<(u64, u128), Vec<&Node>> = HashMap::new();
    for (_, group) in by_size {
        for n in group {
            let h = match hash_file(&n.path, false) {
                Ok(h) => h,
                Err(_) => continue, // vanished or unreadable — skip quietly
            };
            by_head.entry((n.logical, h)).or_default().push(n);
        }
    }
    by_head.retain(|_, v| v.len() > 1);

    // 3. full hash confirmation
    let mut groups = Vec::new();
    for ((size, _), group) in by_head {
        let mut by_full: HashMap<u128, Vec<&Node>> = HashMap::new();
        for n in group {
            let h = match hash_file(&n.path, true) {
                Ok(h) => h,
                Err(_) => continue,
            };
            by_full.entry(h).or_default().push(n);
        }
        for (_, mut files) in by_full {
            if files.len() < 2 {
                continue;
            }
            files.sort_by(|a, b| b.mtime.cmp(&a.mtime).then(a.path.cmp(&b.path)));
            let dups: Vec<DupFile> = files
                .iter()
                .map(|n| DupFile {
                    path: n.path.clone(),
                    logical: n.logical,
                    mtime: n.mtime,
                })
                .collect();
            let keep = dups[0].path.clone();
            groups.push(DuplicateGroup {
                wasted: size * (dups.len() as u64 - 1),
                size,
                files: dups,
                keep,
            });
        }
    }
    groups.sort_by(|a, b| b.wasted.cmp(&a.wasted));
    Ok(groups)
}
