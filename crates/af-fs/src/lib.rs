//! Directory listing for aturfile: one `read_dir`, metadata fetched in
//! parallel, then sorted and filtered in memory so the UI never waits on the
//! disk while the user types or re-sorts.

use std::cmp::Ordering;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use rayon::prelude::*;

pub mod details;
pub mod kind;
pub mod location;
pub mod selection;

pub use kind::FileKind;
pub use selection::Selection;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub name: String,
    pub path: PathBuf,
    pub is_dir: bool,
    pub is_symlink: bool,
    pub hidden: bool,
    /// Bytes for files; 0 for folders (folder sizes come from a scan).
    pub size: u64,
    /// Unix seconds, if the filesystem reported one.
    pub modified: Option<i64>,
}

impl Entry {
    /// Lowercased extension, empty for folders and extensionless files.
    pub fn extension(&self) -> String {
        if self.is_dir {
            return String::new();
        }
        Path::new(&self.name)
            .extension()
            .map(|e| e.to_string_lossy().to_lowercase())
            .unwrap_or_default()
    }

    pub fn kind(&self) -> FileKind {
        kind::classify(&self.name, self.is_dir)
    }
}

/// Read `dir`. Entries that vanish or can't be stat'd mid-listing are skipped
/// rather than failing the whole folder.
pub fn list_dir(dir: &Path) -> io::Result<Vec<Entry>> {
    let raw: Vec<fs::DirEntry> = fs::read_dir(dir)?.filter_map(Result::ok).collect();
    Ok(raw.par_iter().filter_map(entry_from).collect())
}

fn entry_from(de: &fs::DirEntry) -> Option<Entry> {
    let path = de.path();
    let name = de.file_name().to_string_lossy().into_owned();
    let link_md = de.metadata().ok()?;
    let is_symlink = link_md.file_type().is_symlink();
    // Follow symlinks for the kind, so a link to a folder opens like a folder.
    let md = if is_symlink {
        fs::metadata(&path).unwrap_or(link_md)
    } else {
        link_md
    };
    let modified = md
        .modified()
        .ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64);
    Some(Entry {
        hidden: name.starts_with('.') || has_hidden_flag(&md),
        is_dir: md.is_dir(),
        size: if md.is_dir() { 0 } else { md.len() },
        name,
        path,
        is_symlink,
        modified,
    })
}

#[cfg(target_os = "macos")]
fn has_hidden_flag(md: &fs::Metadata) -> bool {
    use std::os::macos::fs::MetadataExt;
    const UF_HIDDEN: u32 = 0x8000;
    md.st_flags() & UF_HIDDEN != 0
}

#[cfg(not(target_os = "macos"))]
fn has_hidden_flag(_md: &fs::Metadata) -> bool {
    false
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum SortKey {
    #[default]
    Name,
    Size,
    Modified,
    Kind,
}

/// Folders first, then by `key`; names break ties so the order is stable.
pub fn sort(entries: &mut [Entry], key: SortKey, ascending: bool) {
    entries.sort_by(|a, b| {
        b.is_dir.cmp(&a.is_dir).then_with(|| {
            let by_key = match key {
                SortKey::Name => Ordering::Equal,
                SortKey::Size => a.size.cmp(&b.size),
                SortKey::Modified => a.modified.cmp(&b.modified),
                SortKey::Kind => a.extension().cmp(&b.extension()),
            };
            let ord = by_key.then_with(|| natural_cmp(&a.name, &b.name));
            if ascending { ord } else { ord.reverse() }
        })
    });
}

/// Finder-style name order: case-insensitive, digit runs compared as numbers
/// ("file2" before "file10").
pub fn natural_cmp(a: &str, b: &str) -> Ordering {
    let (mut a, mut b) = (a.chars().peekable(), b.chars().peekable());
    loop {
        match (a.peek().copied(), b.peek().copied()) {
            (None, None) => return Ordering::Equal,
            (None, Some(_)) => return Ordering::Less,
            (Some(_), None) => return Ordering::Greater,
            (Some(x), Some(y)) if x.is_ascii_digit() && y.is_ascii_digit() => {
                let na = take_number(&mut a);
                let nb = take_number(&mut b);
                // Compare by length after stripping zeros, then lexically:
                // works for numbers longer than u64.
                let (ta, tb) = (na.trim_start_matches('0'), nb.trim_start_matches('0'));
                let ord = ta.len().cmp(&tb.len()).then_with(|| ta.cmp(tb));
                if ord != Ordering::Equal {
                    return ord;
                }
            }
            (Some(x), Some(y)) => {
                let ord = x.to_lowercase().cmp(y.to_lowercase());
                if ord != Ordering::Equal {
                    return ord;
                }
                a.next();
                b.next();
            }
        }
    }
}

fn take_number(it: &mut std::iter::Peekable<std::str::Chars<'_>>) -> String {
    let mut s = String::new();
    while let Some(c) = it.peek().copied().filter(char::is_ascii_digit) {
        s.push(c);
        it.next();
    }
    s
}

/// Indexes of entries whose name contains `query`'s characters in order
/// (case-insensitive), best matches first: prefix, then substring, then
/// scattered. An empty query keeps everything in its current order.
pub fn filter(entries: &[Entry], query: &str) -> Vec<usize> {
    let q = query.to_lowercase();
    if q.is_empty() {
        return (0..entries.len()).collect();
    }
    let mut hits: Vec<(u8, usize)> = entries
        .iter()
        .enumerate()
        .filter_map(|(i, e)| {
            let name = e.name.to_lowercase();
            let rank = if name.starts_with(&q) {
                0
            } else if name.contains(&q) {
                1
            } else if is_subsequence(&q, &name) {
                2
            } else {
                return None;
            };
            Some((rank, i))
        })
        .collect();
    hits.sort_by_key(|&(rank, i)| (rank, i));
    hits.into_iter().map(|(_, i)| i).collect()
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut hay = hay.chars();
    needle.chars().all(|c| hay.any(|h| h == c))
}

/// "1.4 MB" style, decimal units like Finder.
pub fn format_size(bytes: u64) -> String {
    const UNITS: [&str; 6] = ["bytes", "KB", "MB", "GB", "TB", "PB"];
    if bytes < 1000 {
        return format!("{bytes} {}", if bytes == 1 { "byte" } else { "bytes" });
    }
    let mut v = bytes as f64;
    let mut unit = 0;
    while v >= 1000.0 && unit < UNITS.len() - 1 {
        v /= 1000.0;
        unit += 1;
    }
    if v < 10.0 {
        format!("{v:.1} {}", UNITS[unit])
    } else {
        format!("{v:.0} {}", UNITS[unit])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn entry(name: &str, is_dir: bool, size: u64) -> Entry {
        Entry {
            name: name.into(),
            path: PathBuf::from(name),
            is_dir,
            is_symlink: false,
            hidden: name.starts_with('.'),
            size,
            modified: None,
        }
    }

    #[test]
    fn natural_order() {
        let mut v = vec!["file10", "File2", "file1", "a", "file02b"];
        v.sort_by(|a, b| natural_cmp(a, b));
        assert_eq!(v, ["a", "file1", "File2", "file02b", "file10"]);
    }

    #[test]
    fn folders_first_then_size_desc() {
        let mut v = vec![
            entry("small", false, 1),
            entry("dir", true, 0),
            entry("big", false, 99),
        ];
        sort(&mut v, SortKey::Size, false);
        let names: Vec<_> = v.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, ["dir", "big", "small"]);
    }

    #[test]
    fn filter_ranks_prefix_first() {
        let v = vec![
            entry("my_report.pdf", false, 0),
            entry("roleplay", false, 0),
            entry("report.txt", false, 0),
        ];
        let hits: Vec<_> = filter(&v, "rep")
            .iter()
            .map(|&i| v[i].name.clone())
            .collect();
        assert_eq!(hits, ["report.txt", "my_report.pdf", "roleplay"]);
        assert!(filter(&v, "zzz").is_empty());
    }

    #[test]
    fn sizes() {
        assert_eq!(format_size(0), "0 bytes");
        assert_eq!(format_size(1), "1 byte");
        assert_eq!(format_size(1_450_000), "1.4 MB");
        assert_eq!(format_size(52_000_000_000), "52 GB");
    }

    #[test]
    fn lists_a_real_folder() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a.txt"), "hello").unwrap();
        fs::write(dir.path().join(".secret"), "").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        #[cfg(unix)]
        std::os::unix::fs::symlink(dir.path().join("sub"), dir.path().join("link")).unwrap();

        let mut v = list_dir(dir.path()).unwrap();
        sort(&mut v, SortKey::Name, true);
        let a = v.iter().find(|e| e.name == "a.txt").unwrap();
        assert_eq!(a.size, 5);
        assert!(v.iter().find(|e| e.name == ".secret").unwrap().hidden);
        #[cfg(unix)]
        {
            let link = v.iter().find(|e| e.name == "link").unwrap();
            assert!(link.is_symlink && link.is_dir);
        }
        assert!(v[0].is_dir);
    }
}
