//! Parallel directory walker. Never follows symlinks; stays on one filesystem
//! unless told otherwise; counts a hardlinked file once; tracks progress in
//! atomics so a UI can poll mid-scan.

pub use crate::model::ScanResult;
use crate::model::{Kind, Node};
use crate::DbResult;
use parking_lot::Mutex;
use rayon::prelude::*;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::MetadataExt;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone)]
pub struct ScanOptions {
    /// Follow into mounts other than the root's (default: no — keeps `/`
    /// scans out of /Volumes, /private/var etc).
    pub cross_filesystems: bool,
    /// Hard cap on files visited; the result is marked `truncated` when hit.
    pub max_files: Option<u64>,
    /// When false, file nodes are not retained — sizes still accumulate in
    /// parents. Use for cheap size probing (System Data report).
    pub keep_file_nodes: bool,
    /// Deduplicate hardlinks by (dev, ino) — only the first link carries size.
    pub dedupe_hardlinks: bool,
}

impl Default for ScanOptions {
    fn default() -> Self {
        Self {
            cross_filesystems: false,
            max_files: None,
            keep_file_nodes: true,
            dedupe_hardlinks: true,
        }
    }
}

/// Shared counters a UI can poll while the walk runs.
#[derive(Debug, Default)]
pub struct Progress {
    pub files: AtomicU64,
    pub bytes: AtomicU64,
    pub dirs: AtomicU64,
}

impl Progress {
    pub fn snapshot(&self) -> (u64, u64, u64) {
        (
            self.files.load(Ordering::Relaxed),
            self.bytes.load(Ordering::Relaxed),
            self.dirs.load(Ordering::Relaxed),
        )
    }
}

struct Ctx {
    root_dev: u64,
    opts: ScanOptions,
    progress: Option<Arc<Progress>>,
    remaining: AtomicU64,
    truncated: AtomicBool,
    errors: AtomicU64,
    hardlinks: Mutex<HashSet<(u64, u64)>>,
}

impl Ctx {
    /// Budget check for one file; false means the cap is exhausted.
    fn take_file(&self) -> bool {
        loop {
            let cur = self.remaining.load(Ordering::Relaxed);
            if cur == 0 {
                self.truncated.store(true, Ordering::Relaxed);
                return false;
            }
            match self.remaining.compare_exchange_weak(
                cur,
                cur - 1,
                Ordering::Relaxed,
                Ordering::Relaxed,
            ) {
                Ok(_) => return true,
                Err(_) => continue,
            }
        }
    }
}

/// Scan `root` and return the aggregated tree sorted biggest-first.
pub fn scan(
    root: &Path,
    opts: ScanOptions,
    progress: Option<Arc<Progress>>,
) -> DbResult<ScanResult> {
    let start = Instant::now();
    let root = root.canonicalize()?;
    let md = fs::symlink_metadata(&root)?;
    let ctx = Ctx {
        root_dev: md.dev(),
        opts,
        progress,
        remaining: AtomicU64::new(0), // set below (None = max)
        truncated: AtomicBool::new(false),
        errors: AtomicU64::new(0),
        hardlinks: Mutex::new(HashSet::new()),
    };
    ctx.remaining
        .store(ctx.opts.max_files.unwrap_or(u64::MAX), Ordering::Relaxed);

    let mut node = walk_dir(&root, &ctx)?;
    node.sort_by_size();

    Ok(ScanResult {
        root,
        node,
        taken_at: SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs() as i64,
        duration_ms: start.elapsed().as_millis() as u64,
        truncated: ctx.truncated.load(Ordering::Relaxed),
        errors: ctx.errors.load(Ordering::Relaxed),
    })
}

/// Quick subtree size probe: same walk, no per-file nodes retained.
pub fn dir_size(root: &Path) -> (u64, u64, u64) {
    let opts = ScanOptions {
        keep_file_nodes: false,
        dedupe_hardlinks: false,
        ..ScanOptions::default()
    };
    match scan(root, opts, None) {
        Ok(r) => (r.node.logical, r.node.physical, r.node.files),
        Err(_) => (0, 0, 0),
    }
}

fn walk_dir(dir: &Path, ctx: &Ctx) -> DbResult<Node> {
    let mut files: Vec<Node> = Vec::new();
    let mut subdirs: Vec<PathBuf> = Vec::new();
    let mut logical = 0u64;
    let mut physical = 0u64;
    let mut n_files = 0u64;
    let mut n_dirs = 0u64;
    let mut newest = fs::symlink_metadata(dir)?.mtime();

    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(_) => {
            ctx.errors.fetch_add(1, Ordering::Relaxed);
            return Ok(dir_node(
                dir, logical, physical, n_files, n_dirs, newest, files,
            ));
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(_) => {
                ctx.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        let path = entry.path();
        let md = match fs::symlink_metadata(&path) {
            Ok(m) => m,
            Err(_) => {
                ctx.errors.fetch_add(1, Ordering::Relaxed);
                continue;
            }
        };
        // Symlinks contribute zero — we never follow them.
        if md.file_type().is_symlink() {
            continue;
        }
        if md.is_dir() {
            if !ctx.opts.cross_filesystems && md.dev() != ctx.root_dev {
                continue;
            }
            subdirs.push(path);
            continue;
        }
        if !md.is_file() {
            continue; // sockets, fifos, devices
        }
        if !ctx.take_file() {
            continue; // budget exhausted; keep walking dirs for shape
        }
        let (l, p) = if ctx.opts.dedupe_hardlinks && md.nlink() > 1 {
            let seen = ctx.hardlinks.lock().insert((md.dev(), md.ino()));
            if !seen {
                (0, 0)
            } else {
                (md.len(), md.blocks() * 512)
            }
        } else {
            (md.len(), md.blocks() * 512)
        };
        logical += l;
        physical += p;
        n_files += 1;
        newest = newest.max(md.mtime());
        if let Some(prog) = &ctx.progress {
            prog.files.fetch_add(1, Ordering::Relaxed);
            prog.bytes.fetch_add(l, Ordering::Relaxed);
        }
        if ctx.opts.keep_file_nodes {
            files.push(Node {
                name: path
                    .file_name()
                    .unwrap_or_default()
                    .to_string_lossy()
                    .into_owned(),
                path,
                kind: Kind::File,
                logical: l,
                physical: p,
                files: 1,
                dirs: 0,
                mtime: md.mtime(),
                children: Vec::new(),
            });
        }
    }

    // Recurse in parallel. Rayon steals work across the whole subtree.
    let child_nodes: Vec<Node> = subdirs
        .par_iter()
        .map(|d| walk_dir(d, ctx))
        .collect::<DbResult<Vec<Node>>>()?;
    for c in &child_nodes {
        logical += c.logical;
        physical += c.physical;
        n_files += c.files;
        n_dirs += c.dirs;
        newest = newest.max(c.mtime);
    }
    n_dirs += subdirs.len() as u64;
    ctx.progress
        .as_ref()
        .map(|p| p.dirs.fetch_add(subdirs.len() as u64, Ordering::Relaxed));

    let mut children = files;
    children.extend(child_nodes);
    Ok(dir_node(
        dir, logical, physical, n_files, n_dirs, newest, children,
    ))
}

fn dir_node(
    path: &Path,
    logical: u64,
    physical: u64,
    files: u64,
    dirs: u64,
    mtime: i64,
    children: Vec<Node>,
) -> Node {
    Node {
        name: path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .into_owned(),
        path: path.to_path_buf(),
        kind: Kind::Dir,
        logical,
        physical,
        files,
        dirs,
        mtime,
        children,
    }
}
