//! What the preview pane shows about an item: metadata, a bounded folder
//! summary and a capped text read. All of it runs off the UI thread.

use std::fs;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Details {
    pub size: u64,
    pub is_dir: bool,
    pub is_symlink: bool,
    /// Where a symlink points.
    pub link_target: Option<PathBuf>,
    /// Unix seconds.
    pub created: Option<i64>,
    pub modified: Option<i64>,
    pub accessed: Option<i64>,
    /// Unix permission bits (`0o755`), when the platform has them.
    pub mode: Option<u32>,
    pub readonly: bool,
}

fn secs(t: io::Result<SystemTime>) -> Option<i64> {
    t.ok()
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_secs() as i64)
}

/// Metadata for `path`, following symlinks for everything but the link
/// itself.
pub fn details(path: &Path) -> io::Result<Details> {
    let link = fs::symlink_metadata(path)?;
    let is_symlink = link.file_type().is_symlink();
    let md = if is_symlink {
        fs::metadata(path).unwrap_or(link)
    } else {
        link
    };
    #[cfg(unix)]
    let mode = {
        use std::os::unix::fs::PermissionsExt;
        Some(md.permissions().mode())
    };
    #[cfg(not(unix))]
    let mode = None;
    Ok(Details {
        size: if md.is_dir() { 0 } else { md.len() },
        is_dir: md.is_dir(),
        is_symlink,
        link_target: if is_symlink {
            fs::read_link(path).ok()
        } else {
            None
        },
        created: secs(md.created()),
        modified: secs(md.modified()),
        accessed: secs(md.accessed()),
        mode,
        readonly: md.permissions().readonly(),
    })
}

/// `drwxr-xr-x` from permission bits.
pub fn permissions_string(mode: u32, is_dir: bool) -> String {
    let mut s = String::with_capacity(10);
    s.push(if is_dir { 'd' } else { '-' });
    for shift in [6, 3, 0] {
        let bits = (mode >> shift) & 0o7;
        s.push(if bits & 4 != 0 { 'r' } else { '-' });
        s.push(if bits & 2 != 0 { 'w' } else { '-' });
        s.push(if bits & 1 != 0 { 'x' } else { '-' });
    }
    s
}

/// A folder's contents, counted up to a limit so huge trees stay quick.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct FolderSummary {
    /// Direct children.
    pub items: u64,
    /// Files and folders under it, recursively.
    pub files: u64,
    pub folders: u64,
    pub bytes: u64,
    /// The walk stopped at the limit: totals are a lower bound.
    pub truncated: bool,
}

/// Count `dir`'s direct children, then walk it (not following symlinks)
/// until `limit` entries have been seen.
pub fn summarize(dir: &Path, limit: u64) -> io::Result<FolderSummary> {
    let mut sum = FolderSummary {
        items: fs::read_dir(dir)?.count() as u64,
        ..FolderSummary::default()
    };
    let mut stack = vec![dir.to_path_buf()];
    let mut seen = 0u64;
    while let Some(d) = stack.pop() {
        let Ok(rd) = fs::read_dir(&d) else { continue };
        for e in rd.flatten() {
            seen += 1;
            if seen > limit {
                sum.truncated = true;
                return Ok(sum);
            }
            let Ok(ft) = e.file_type() else { continue };
            if ft.is_dir() {
                sum.folders += 1;
                stack.push(e.path());
            } else {
                sum.files += 1;
                if let Ok(md) = e.metadata() {
                    sum.bytes += md.len();
                }
            }
        }
    }
    Ok(sum)
}

/// The start of a file for the preview pane.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPreview {
    pub text: String,
    /// The file is longer than what was read.
    pub truncated: bool,
    /// NUL bytes near the start: not text.
    pub binary: bool,
}

/// Read up to `cap` bytes of `path` as (lossy) UTF-8.
pub fn read_text(path: &Path, cap: usize) -> io::Result<TextPreview> {
    let mut f = fs::File::open(path)?;
    let mut buf = Vec::with_capacity(cap.min(64 * 1024));
    f.by_ref().take(cap as u64 + 1).read_to_end(&mut buf)?;
    let truncated = buf.len() > cap;
    buf.truncate(cap);
    let binary = buf.iter().take(8192).any(|&b| b == 0);
    if binary {
        return Ok(TextPreview {
            text: String::new(),
            truncated,
            binary,
        });
    }
    // Don't cut a UTF-8 sequence in half at the cap.
    let mut text = String::from_utf8_lossy(&buf).into_owned();
    if truncated && text.ends_with('\u{fffd}') {
        text.pop();
    }
    Ok(TextPreview {
        text,
        truncated,
        binary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn permission_strings() {
        assert_eq!(permissions_string(0o755, true), "drwxr-xr-x");
        assert_eq!(permissions_string(0o640, false), "-rw-r-----");
    }

    #[test]
    fn summarizes_and_limits() {
        let dir = tempfile::tempdir().unwrap();
        fs::write(dir.path().join("a"), "12345").unwrap();
        fs::create_dir(dir.path().join("sub")).unwrap();
        fs::write(dir.path().join("sub/b"), "123").unwrap();
        let s = summarize(dir.path(), 1000).unwrap();
        assert_eq!((s.items, s.files, s.folders, s.bytes), (2, 2, 1, 8));
        assert!(!s.truncated);
        assert!(summarize(dir.path(), 1).unwrap().truncated);
    }

    #[test]
    fn reads_capped_text() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("t.txt");
        fs::write(&p, "hello world").unwrap();
        let t = read_text(&p, 5).unwrap();
        assert_eq!(t.text, "hello");
        assert!(t.truncated && !t.binary);
        let t = read_text(&p, 100).unwrap();
        assert_eq!(t.text, "hello world");
        assert!(!t.truncated);
        fs::write(&p, b"\x00\x01binary").unwrap();
        assert!(read_text(&p, 100).unwrap().binary);
        let d = details(&p).unwrap();
        assert_eq!(d.size, 8);
        assert!(!d.is_dir);
    }
}
