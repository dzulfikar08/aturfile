//! The index: every name under the roots, breadth first (so shallow items
//! are there first), on its own thread. Readers see it grow.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Duration, Instant};

use parking_lot::RwLock;

/// One indexed file or folder.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    pub path: String,
    name_at: usize,
    pub is_dir: bool,
}

impl Item {
    pub fn new(path: String, is_dir: bool) -> Self {
        let trimmed = path.trim_end_matches('/');
        let name_at = trimmed.rfind('/').map_or(0, |i| i + 1);
        Self {
            path,
            name_at,
            is_dir,
        }
    }

    pub fn name(&self) -> &str {
        self.path[self.name_at..].trim_end_matches('/')
    }

    /// The folder holding the item.
    pub fn parent(&self) -> &str {
        let p = &self.path[..self.name_at];
        if p.len() > 1 {
            p.trim_end_matches('/')
        } else {
            p
        }
    }
}

#[derive(Debug, Clone)]
pub struct IndexOptions {
    pub roots: Vec<PathBuf>,
    /// Folders not to enter (`~/Library` by default in the app).
    pub skip: Vec<PathBuf>,
    pub max_entries: usize,
    pub max_time: Duration,
    /// Index names starting with a dot.
    pub include_hidden: bool,
}

impl Default for IndexOptions {
    fn default() -> Self {
        Self {
            roots: Vec::new(),
            skip: Vec::new(),
            max_entries: 200_000,
            max_time: Duration::from_secs(10),
            include_hidden: false,
        }
    }
}

/// Where the index is.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct IndexState {
    pub count: usize,
    pub done: bool,
    /// A limit (time or entries) stopped the walk: results are partial.
    pub partial: bool,
}

/// Bundles shown as one item, never entered.
const PACKAGES: &[&str] = &[
    "app",
    "bundle",
    "framework",
    "photoslibrary",
    "musiclibrary",
    "xcodeproj",
    "xcworkspace",
    "plugin",
    "kext",
    "pages",
    "numbers",
    "key",
];

/// Folders full of generated files nobody searches by name.
const SKIP_NAMES: &[&str] = &["node_modules"];

#[cfg(target_os = "macos")]
fn has_hidden_flag(path: &Path) -> bool {
    use std::os::macos::fs::MetadataExt;
    const UF_HIDDEN: u32 = 0x8000;
    std::fs::symlink_metadata(path).is_ok_and(|m| m.st_flags() & UF_HIDDEN != 0)
}

#[cfg(not(target_os = "macos"))]
fn has_hidden_flag(_path: &Path) -> bool {
    false
}

#[derive(Default)]
pub struct Index {
    items: RwLock<Vec<Item>>,
    done: AtomicBool,
    partial: AtomicBool,
    cancel: AtomicBool,
}

impl Index {
    /// Start indexing on a new thread.
    pub fn start(opts: IndexOptions) -> Arc<Self> {
        let index = Arc::new(Self::default());
        let worker = index.clone();
        let spawned = std::thread::Builder::new()
            .name("af-search-index".into())
            .spawn(move || worker.build(&opts));
        if spawned.is_err() {
            index.done.store(true, Ordering::Relaxed);
        }
        index
    }

    /// Walk the roots on this thread (what `start` runs).
    pub fn build(&self, opts: &IndexOptions) {
        let started = Instant::now();
        let mut queue: VecDeque<PathBuf> = opts.roots.iter().cloned().collect();
        let mut batch: Vec<Item> = Vec::with_capacity(2048);
        let mut total = 0usize;
        'walk: while let Some(dir) = queue.pop_front() {
            let Ok(rd) = std::fs::read_dir(&dir) else {
                continue;
            };
            for entry in rd.flatten() {
                if self.cancel.load(Ordering::Relaxed) {
                    break 'walk;
                }
                if total >= opts.max_entries || started.elapsed() > opts.max_time {
                    self.partial.store(true, Ordering::Relaxed);
                    break 'walk;
                }
                let name = entry.file_name();
                let name = name.to_string_lossy();
                if !opts.include_hidden && name.starts_with('.') {
                    continue;
                }
                let Ok(ft) = entry.file_type() else { continue };
                let path = entry.path();
                let is_dir = ft.is_dir();
                // Folders macOS hides (the UF_HIDDEN flag, as Finder does) are
                // skipped with their contents; files are not stat'd for speed.
                if is_dir && !opts.include_hidden && has_hidden_flag(&path) {
                    continue;
                }
                if is_dir
                    && !opts.skip.iter().any(|s| s == &path)
                    && !SKIP_NAMES.contains(&name.as_ref())
                    && !is_package(&path)
                {
                    queue.push_back(path.clone());
                }
                batch.push(Item::new(path.to_string_lossy().into_owned(), is_dir));
                total += 1;
                if batch.len() >= 2048 {
                    self.items.write().append(&mut batch);
                }
            }
        }
        self.items.write().append(&mut batch);
        self.done.store(true, Ordering::Relaxed);
    }

    /// Stop the walk (results so far stay).
    pub fn cancel(&self) {
        self.cancel.store(true, Ordering::Relaxed);
    }

    pub fn state(&self) -> IndexState {
        IndexState {
            count: self.items.read().len(),
            done: self.done.load(Ordering::Relaxed),
            partial: self.partial.load(Ordering::Relaxed),
        }
    }

    /// Read the items indexed so far.
    pub fn with_items<R>(&self, f: impl FnOnce(&[Item]) -> R) -> R {
        f(&self.items.read())
    }
}

fn is_package(path: &Path) -> bool {
    path.extension()
        .map(|e| PACKAGES.contains(&e.to_string_lossy().to_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn items_know_name_and_parent() {
        let i = Item::new("/Users/me/notes.txt".into(), false);
        assert_eq!(i.name(), "notes.txt");
        assert_eq!(i.parent(), "/Users/me");
        assert_eq!(Item::new("/a".into(), true).parent(), "/");
    }

    #[test]
    fn indexes_skips_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        fs::create_dir_all(d.join("Docs/deep/deeper")).unwrap();
        fs::create_dir_all(d.join("Library/Caches")).unwrap();
        fs::create_dir_all(d.join("Thing.app/Contents")).unwrap();
        fs::create_dir_all(d.join("node_modules/pkg")).unwrap();
        fs::write(d.join("Docs/deep/deeper/plan.md"), "").unwrap();
        fs::write(d.join(".secret"), "").unwrap();
        let opts = IndexOptions {
            roots: vec![d.to_path_buf()],
            skip: vec![d.join("Library")],
            ..IndexOptions::default()
        };
        let index = Index::default();
        index.build(&opts);
        let st = index.state();
        assert!(st.done && !st.partial);
        let names: Vec<String> =
            index.with_items(|v| v.iter().map(|i| i.name().to_owned()).collect());
        assert!(names.contains(&"plan.md".to_owned()));
        assert!(
            names.contains(&"Library".to_owned()),
            "the folder itself is listed"
        );
        assert!(!names.contains(&"Caches".to_owned()), "but not entered");
        assert!(names.contains(&"Thing.app".to_owned()));
        assert!(!names.contains(&"Contents".to_owned()));
        assert!(!names.contains(&"pkg".to_owned()));
        assert!(!names.contains(&".secret".to_owned()));
        // Breadth first: shallow items come before deep ones.
        let pos = |n: &str| names.iter().position(|x| x == n).unwrap();
        assert!(pos("Docs") < pos("plan.md"));

        let limited = Index::default();
        limited.build(&IndexOptions {
            max_entries: 2,
            ..opts.clone()
        });
        let st = limited.state();
        assert!(st.partial && st.count == 2);

        let threaded = Index::start(opts);
        while !threaded.state().done {
            std::thread::sleep(Duration::from_millis(5));
        }
        assert!(threaded.state().count >= 6);
    }
}
