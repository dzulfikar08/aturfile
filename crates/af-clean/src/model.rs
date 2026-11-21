use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Kind {
    Dir,
    File,
}

/// One filesystem entry. Directory aggregates are computed bottom-up during
/// the scan: `logical`/`physical` are sums of their subtree, `mtime` is the
/// newest mtime in the subtree.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub name: String,
    pub path: PathBuf,
    pub kind: Kind,
    /// Logical size: `st_size` for files, subtree sum for dirs.
    pub logical: u64,
    /// Space on disk: `st_blocks * 512` (reflects APFS compression and
    /// sparse files — `logical` minus this is what compression saved).
    pub physical: u64,
    pub files: u64,
    pub dirs: u64,
    /// Unix epoch seconds. Dirs carry the max mtime under them.
    pub mtime: i64,
    pub children: Vec<Node>,
}

impl Node {
    pub fn is_dir(&self) -> bool {
        self.kind == Kind::Dir
    }

    /// Sort children biggest-first, recursively. Deterministic (ties by name).
    pub fn sort_by_size(&mut self) {
        self.children
            .sort_by(|a, b| b.logical.cmp(&a.logical).then(a.name.cmp(&b.name)));
        for c in &mut self.children {
            c.sort_by_size();
        }
    }

    /// Depth-first walk of this subtree.
    pub fn walk<'a>(&'a self, f: &mut impl FnMut(&'a Node)) {
        f(self);
        for c in &self.children {
            c.walk(f);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ScanResult {
    pub root: PathBuf,
    pub node: Node,
    /// Unix epoch seconds when the scan finished.
    pub taken_at: i64,
    pub duration_ms: u64,
    /// True if `max_files` cut the walk short.
    pub truncated: bool,
    /// Entries that could not be stat'd/read (permission errors etc).
    pub errors: u64,
}
