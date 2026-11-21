//! Paths as the user types and reads them: the location field (`~`
//! expansion, relative paths) and the breadcrumb path bar.

use std::path::{Component, Path, PathBuf};

/// Resolve what was typed in the location field. `~` and `~/x` are the
/// home folder, relative paths start at `cwd`, and `.`/`..` are folded
/// without touching the disk. Quotes around a pasted path are dropped.
pub fn expand(input: &str, cwd: &Path, home: &Path) -> PathBuf {
    let s = input.trim();
    let s = s
        .strip_prefix('"')
        .and_then(|r| r.strip_suffix('"'))
        .or_else(|| s.strip_prefix('\'').and_then(|r| r.strip_suffix('\'')))
        .unwrap_or(s);
    let raw = if s.is_empty() {
        cwd.to_path_buf()
    } else if s == "~" {
        home.to_path_buf()
    } else if let Some(rest) = s.strip_prefix("~/") {
        home.join(rest)
    } else if s.starts_with('/') {
        PathBuf::from(s)
    } else {
        cwd.join(s)
    };
    normalize(&raw)
}

/// Fold `.` and `..` lexically (`/a/b/../c` → `/a/c`); `..` at the root stays at the root.
pub fn normalize(path: &Path) -> PathBuf {
    let mut out = PathBuf::new();
    for comp in path.components() {
        match comp {
            Component::CurDir => {}
            Component::ParentDir => {
                if out.has_root() && out.parent().is_none() {
                    // `..` at the root stays there.
                } else if matches!(
                    out.components().next_back(),
                    None | Some(Component::ParentDir)
                ) {
                    out.push("..");
                } else {
                    out.pop();
                }
            }
            other => out.push(other),
        }
    }
    if out.as_os_str().is_empty() {
        out.push(if path.is_absolute() { "/" } else { "." });
    }
    out
}

/// `~/Documents` for paths in `home`, the full path otherwise.
pub fn display(path: &Path, home: &Path) -> String {
    if path == home {
        return "~".to_owned();
    }
    match path.strip_prefix(home) {
        Ok(rest) if home != Path::new("/") => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}

/// One segment of the path bar.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Crumb {
    pub label: String,
    pub path: PathBuf,
}

/// The path bar's segments: from Home (`~`) for paths inside it, from the
/// root (`/`) otherwise.
pub fn breadcrumbs(path: &Path, home: &Path) -> Vec<Crumb> {
    let mut out = Vec::new();
    let mut acc = PathBuf::new();
    for comp in path.components() {
        acc.push(comp);
        let label = if acc == home {
            out.clear();
            "~".to_owned()
        } else {
            match comp {
                Component::RootDir => "/".to_owned(),
                other => other.as_os_str().to_string_lossy().into_owned(),
            }
        };
        out.push(Crumb {
            label,
            path: acc.clone(),
        });
    }
    out
}

/// Shorten a long path bar to the first crumb, an ellipsis (which opens the
/// folder above the kept tail) and the last `keep` crumbs.
pub fn collapse(mut crumbs: Vec<Crumb>, keep: usize) -> Vec<Crumb> {
    let keep = keep.max(1);
    if crumbs.len() <= keep + 2 {
        return crumbs;
    }
    let tail = crumbs.split_off(crumbs.len() - keep);
    crumbs.truncate(1);
    let above = tail[0]
        .path
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_default();
    crumbs.push(Crumb {
        label: "\u{2026}".to_owned(),
        path: above,
    });
    crumbs.extend(tail);
    crumbs
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(c: &[Crumb]) -> Vec<&str> {
        c.iter().map(|c| c.label.as_str()).collect()
    }

    #[test]
    fn expands_home_and_relative_paths() {
        let home = Path::new("/Users/me");
        let cwd = Path::new("/Users/me/Documents");
        assert_eq!(expand("~", cwd, home), PathBuf::from("/Users/me"));
        assert_eq!(
            expand("~/Downloads", cwd, home),
            PathBuf::from("/Users/me/Downloads")
        );
        assert_eq!(
            expand("  notes ", cwd, home),
            PathBuf::from("/Users/me/Documents/notes")
        );
        assert_eq!(expand("..", cwd, home), PathBuf::from("/Users/me"));
        assert_eq!(expand("/tmp/./a/../b", cwd, home), PathBuf::from("/tmp/b"));
        assert_eq!(expand("/..", cwd, home), PathBuf::from("/"));
        assert_eq!(normalize(Path::new("a/../../b")), PathBuf::from("../b"));
        assert_eq!(normalize(Path::new("a/..")), PathBuf::from("."));
        assert_eq!(expand("", cwd, home), cwd.to_path_buf());
        assert_eq!(
            expand("\"/Volumes/My Disk\"", cwd, home),
            PathBuf::from("/Volumes/My Disk")
        );
        // "~user" is not expanded: it's a relative name.
        assert_eq!(
            expand("~bob", cwd, home),
            PathBuf::from("/Users/me/Documents/~bob")
        );
    }

    #[test]
    fn crumbs_start_at_home_or_root() {
        let home = Path::new("/Users/me");
        let c = breadcrumbs(Path::new("/Users/me/Documents/aturfile"), home);
        assert_eq!(labels(&c), ["~", "Documents", "aturfile"]);
        assert_eq!(c[1].path, PathBuf::from("/Users/me/Documents"));
        let c = breadcrumbs(Path::new("/Volumes/Disk"), home);
        assert_eq!(labels(&c), ["/", "Volumes", "Disk"]);
        assert_eq!(labels(&breadcrumbs(Path::new("/"), home)), ["/"]);
    }

    #[test]
    fn long_paths_collapse() {
        let home = Path::new("/Users/me");
        let c = breadcrumbs(Path::new("/Users/me/a/b/c/d/e/f"), home);
        let short = collapse(c, 3);
        assert_eq!(labels(&short), ["~", "\u{2026}", "d", "e", "f"]);
        assert_eq!(short[1].path, PathBuf::from("/Users/me/a/b/c"));
        let c = breadcrumbs(Path::new("/Users/me/a/b"), home);
        assert_eq!(collapse(c.clone(), 3), c);
    }

    #[test]
    fn displays_home_relative() {
        let home = Path::new("/Users/me");
        assert_eq!(display(Path::new("/Users/me"), home), "~");
        assert_eq!(display(Path::new("/Users/me/x"), home), "~/x");
        assert_eq!(display(Path::new("/tmp"), home), "/tmp");
    }
}
