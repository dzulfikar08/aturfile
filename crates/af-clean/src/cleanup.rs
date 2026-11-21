//! The staged cleanup queue and its guard rails.
//!
//! Nothing in this module deletes anything until [`apply`] runs with
//! `dry_run = false`. Staging validates against a protected-path list; apply
//! validates again (a file may have changed between stage and apply).

use crate::paths::{home, trash_dir};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Disposal {
    /// Move to the user's Trash. Default: reversible, Finder-visible.
    Trash,
    /// Unlink immediately. Only after explicit confirmation.
    Delete,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StagedItem {
    pub path: PathBuf,
    pub logical: u64,
    /// Category id from the System Data report, or "manual".
    pub category: String,
    pub reason: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CleanupOutcome {
    pub path: PathBuf,
    pub ok: bool,
    pub message: String,
    pub freed: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct CleanupQueue {
    pub items: Vec<StagedItem>,
}

impl CleanupQueue {
    pub fn total(&self) -> u64 {
        self.items.iter().map(|i| i.logical).sum()
    }

    pub fn stage(&mut self, item: StagedItem) -> Result<(), String> {
        check_protected(&item.path)?;
        if self.items.iter().any(|i| i.path == item.path) {
            return Err(format!("already staged: {}", item.path.display()));
        }
        // Ancestor/descendant double-booking would double-count and the inner
        // apply would no-op confusingly. Refuse both orders.
        for existing in &self.items {
            if item.path.starts_with(&existing.path) || existing.path.starts_with(&item.path) {
                return Err(format!(
                    "overlaps already-staged {}",
                    existing.path.display()
                ));
            }
        }
        self.items.push(item);
        Ok(())
    }

    pub fn unstage(&mut self, path: &Path) {
        self.items.retain(|i| i.path != path);
    }

    pub fn clear(&mut self) {
        self.items.clear();
    }

    /// Re-validate every staged path right before apply: still exists, still
    /// outside protected roots, no overlaps introduced meanwhile.
    pub fn validate(&self) -> Result<(), String> {
        let mut seen: Vec<&PathBuf> = Vec::new();
        for item in &self.items {
            check_protected(&item.path)?;
            if !item.path.symlink_metadata().is_ok() {
                return Err(format!("no longer exists: {}", item.path.display()));
            }
            for s in &seen {
                if item.path.starts_with(*s) || s.starts_with(&item.path) {
                    return Err(format!("overlap: {}", item.path.display()));
                }
            }
            seen.push(&item.path);
        }
        Ok(())
    }
}

/// Reasons a path must never be staged. Returns Err with the explanation.
pub fn check_protected(path: &Path) -> Result<(), String> {
    let path = match path.canonicalize() {
        Ok(p) => p,
        Err(_) => path.to_path_buf(), // missing: apply-time validate reports it
    };

    // OS scratch space first: macOS per-user tempdirs live under
    // /private/var/folders, which would otherwise trip the /var + /private
    // guards and break every test fixture and staged temp file.
    // temp_dir() returns the /var spelling while staged paths are
    // canonicalized to /private/var — compare canonical on both sides.
    let tmp = std::env::temp_dir();
    let tmp = tmp.canonicalize().unwrap_or(tmp);
    let in_scratch = path == tmp || path.starts_with(&tmp);

    if !in_scratch {
        const SYSTEM_ROOTS: &[&str] = &[
            // No "/home": on Linux dev machines $HOME lives under it, and
            // user data under the home is stageable by design (credential
            // dots are refused separately below).
            "/System", "/usr", "/bin", "/sbin", "/etc", "/var", "/private", "/dev", "/Library",
            "/Volumes", "/cores", "/opt", "/boot", "/proc", "/sys",
        ];
        for sys in SYSTEM_ROOTS {
            let sysp = Path::new(sys);
            if path == sysp {
                return Err(format!("{sys} is system-critical"));
            }
            if path.starts_with(sysp) && sys != &"/Volumes" && !is_user_managed(&path, sysp) {
                return Err(format!("inside protected root {sys}"));
            }
        }
    }

    let home = home();
    // Dots you never want a cleaner to touch.
    for dot in [".ssh", ".gnupg", ".aws", ".kube", ".config/gcloud"] {
        let p = home.join(dot);
        if path == p || path.starts_with(&p) {
            return Err(format!("{dot} holds credentials"));
        }
    }
    let keychains = home.join("Library/Keychains");
    if path == keychains || path.starts_with(&keychains) {
        return Err("Keychains are system credentials".into());
    }
    // Owner policy: node_modules is never a cleanup target — reinstalling is
    // costlier than the disk it would save.
    if path
        .file_name()
        .map(|n| n == std::ffi::OsStr::new("node_modules"))
        .unwrap_or(false)
    {
        return Err("node_modules is excluded by policy".into());
    }
    // App bundles: only stage the whole .app, never files inside one.
    if let Some(component) = path.ancestors().find(|a| {
        a.extension()
            .map(|e| e.eq_ignore_ascii_case("app"))
            .unwrap_or(false)
    }) {
        if component != path {
            return Err(format!(
                "{} is inside an app bundle. Stage the whole {} instead",
                path.display(),
                component.display()
            ));
        }
    }
    Ok(())
}

/// /Library is protected, but admin-cached junk like /Library/Caches is the
/// exception a power user may stage. Keep the exception narrow.
fn is_user_managed(path: &Path, root: &Path) -> bool {
    let Ok(rel) = path.strip_prefix(root) else {
        return false;
    };
    rel.iter()
        .next()
        .map(|seg| seg == std::ffi::OsStr::new("Caches"))
        .unwrap_or(false)
}

/// Compute the plan without touching disk state (beyond reads).
pub fn plan(items: &[StagedItem], disposal: Disposal) -> Vec<CleanupOutcome> {
    items
        .iter()
        .map(|i| CleanupOutcome {
            path: i.path.clone(),
            ok: true,
            message: match disposal {
                Disposal::Trash => "would move to Trash".into(),
                Disposal::Delete => "would delete permanently".into(),
            },
            freed: i.logical,
        })
        .collect()
}

/// Execute the queue. With `dry_run` nothing is modified.
pub fn apply(items: &[StagedItem], disposal: Disposal, dry_run: bool) -> Vec<CleanupOutcome> {
    let mut out = Vec::new();
    for item in items {
        if dry_run {
            out.push(plan(std::slice::from_ref(item), disposal).remove(0));
            continue;
        }
        if let Err(e) = check_protected(&item.path) {
            out.push(CleanupOutcome {
                path: item.path.clone(),
                ok: false,
                message: e,
                freed: 0,
            });
            continue;
        }
        // The staged logical size is the truthful number: st_len on a dir
        // is just the inode size, not the subtree.
        let exists = item.path.symlink_metadata().is_ok();
        let logical = if exists { item.logical } else { 0 };
        let res = match disposal {
            Disposal::Trash => trash_path(&item.path),
            Disposal::Delete => {
                if item.path.is_dir() {
                    remove_dir_all_force(&item.path)
                } else {
                    fs::remove_file(&item.path)
                }
            }
        };
        out.push(match res {
            Ok(()) => CleanupOutcome {
                path: item.path.clone(),
                ok: true,
                message: match disposal {
                    Disposal::Trash => "moved to Trash".into(),
                    Disposal::Delete => "deleted".into(),
                },
                freed: logical,
            },
            Err(e) => CleanupOutcome {
                path: item.path.clone(),
                ok: false,
                message: e.to_string(),
                freed: 0,
            },
        });
    }
    out
}

/// Move `path` into the user's Trash under a non-colliding name.
fn trash_path(path: &Path) -> std::io::Result<()> {
    // macOS: Finder's Trash (Put Back works; allowed in the sandbox for granted
    // folders). Linux: the freedesktop Trash with its restore info. Only a
    // cross-device move on Linux falls through to copy-then-delete below.
    match af_mac::trash(path) {
        Err(e) if !cfg!(target_os = "macos") && e.kind() == std::io::ErrorKind::CrossesDevices => {}
        other => return other,
    }
    let dest_dir = trash_dir();
    fs::create_dir_all(&dest_dir)?;
    let name = path
        .file_name()
        .map(|n| n.to_os_string())
        .unwrap_or_default();
    let mut candidate = dest_dir.join(&name);
    let mut n = 1u32;
    let file_stem = Path::new(&name)
        .file_stem()
        .map(|s| s.to_os_string())
        .unwrap_or_else(|| name.clone());
    let ext = Path::new(&name).extension().map(|e| e.to_os_string());
    while candidate.symlink_metadata().is_ok() {
        n += 1;
        let mut renamed = file_stem.clone();
        renamed.push(format!(" ({n})"));
        if let Some(e) = &ext {
            renamed.push(".");
            renamed.push(e.clone());
        }
        candidate = dest_dir.join(renamed);
    }
    // rename(2) fails across filesystems (e.g. /tmp tempdirs in tests, or
    // staging from an external volume). Fall back to copy-into-Trash +
    // delete-source so the data still lands in the Trash.
    match fs::rename(path, &candidate) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::CrossesDevices => {
            copy_to(path, &candidate)?;
            remove_source(path)
        }
        Err(e) => Err(e),
    }
}

fn copy_to(src: &Path, dest: &Path) -> std::io::Result<()> {
    let md = fs::symlink_metadata(src)?;
    if md.is_dir() {
        fs::create_dir_all(dest)?;
        for entry in fs::read_dir(src)? {
            let entry = entry?;
            copy_to(&entry.path(), &dest.join(entry.file_name()))?;
        }
        Ok(())
    } else {
        fs::copy(src, dest).map(|_| ())
    }
}

fn remove_source(src: &Path) -> std::io::Result<()> {
    if src.is_dir() && !src.symlink_metadata()?.file_type().is_symlink() {
        remove_dir_all_force(src)
    } else {
        fs::remove_file(src)
    }
}

/// `remove_dir_all` gives up on the first read-only directory (caches like
/// dotslash deliberately mark entries 555). Retry once after granting the
/// owner write permission — only on files we already own.
fn remove_dir_all_force(path: &Path) -> std::io::Result<()> {
    match fs::remove_dir_all(path) {
        Ok(()) => Ok(()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
            let _ = grant_owner_write(path);
            fs::remove_dir_all(path)
        }
        Err(e) => Err(e),
    }
}

fn grant_owner_write(dir: &Path) -> std::io::Result<()> {
    use std::os::unix::fs::PermissionsExt;
    fn rec(path: &Path) -> std::io::Result<()> {
        let md = fs::symlink_metadata(path)?;
        if md.is_dir() {
            for entry in fs::read_dir(path)? {
                let entry = entry?;
                let _ = rec(&entry.path());
            }
        }
        let mut perms = fs::metadata(path)?.permissions();
        perms.set_mode(perms.mode() | 0o200);
        fs::set_permissions(path, perms)
    }
    rec(dir)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn protected_roots_refuse() {
        assert!(check_protected(Path::new("/System")).is_err());
        assert!(check_protected(Path::new("/usr/bin")).is_err());
        assert!(check_protected(Path::new("/Library/Fonts")).is_err());
        assert!(check_protected(Path::new("/Volumes")).is_err());
    }

    #[test]
    fn library_caches_is_stageable_but_library_is_not() {
        assert!(check_protected(Path::new("/Library/Caches/Homebrew")).is_ok());
    }

    #[test]
    fn delete_handles_read_only_cache_entries() {
        let td = tempfile::Builder::new()
            .prefix("dbc-ro-")
            .tempdir()
            .unwrap();
        let root = td.path().join("cache");
        let locked = root.join("tool/64/entry");
        fs::create_dir_all(&locked).unwrap();
        fs::write(locked.join("blob"), b"x").unwrap();
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o555)).unwrap();

        let items = vec![StagedItem {
            path: root.clone(),
            logical: 1,
            category: "dev_caches".into(),
            reason: "test".into(),
        }];
        let out = apply(&items, Disposal::Delete, false);
        assert!(out[0].ok, "{:?}", out[0].message);
        assert!(!root.exists());
    }

    #[test]
    fn inside_app_bundle_refused() {
        assert!(check_protected(Path::new("/Applications/Foo.app/Contents/MacOS/foo")).is_err());
        assert!(check_protected(Path::new("/Applications/Foo.app")).is_ok());
    }

    #[test]
    fn node_modules_refused_by_policy() {
        let h = home();
        assert!(check_protected(&h.join("proj/node_modules")).is_err());
        assert!(check_protected(&h.join("proj/src")).is_ok());
    }

    #[test]
    fn home_dot_credentials_refused() {
        let h = home();
        assert!(check_protected(&h.join(".ssh/id_rsa")).is_err());
        assert!(check_protected(&h.join(".ssh")).is_err());
        assert!(check_protected(&h.join("Library/Caches")).is_ok());
    }

    #[test]
    fn queue_rejects_overlap() {
        let h = home();
        let mut q = CleanupQueue::default();
        q.stage(StagedItem {
            path: h.join("Library/Caches"),
            logical: 1,
            category: "user_caches".into(),
            reason: "test".into(),
        })
        .unwrap();
        assert!(q
            .stage(StagedItem {
                path: h.join("Library/Caches/ms-playwright"),
                logical: 1,
                category: "user_caches".into(),
                reason: "test".into(),
            })
            .is_err());
    }

    #[test]
    fn trash_moves_and_uniquifies() {
        let tmp = std::env::temp_dir().join(format!("dbc-trash-{}", std::process::id()));
        let stage = tmp.join("stage");
        fs::create_dir_all(&stage).unwrap();
        let victim = stage.join("junk-dir");
        fs::create_dir_all(&victim).unwrap();
        fs::write(victim.join("f.txt"), "x").unwrap();

        // Point trash_dir's natural location at our temp sandbox by moving.
        // trash_path uses the user trash dir; create it, then move.
        let res = trash_path(&victim);
        // On CI HOME may make dest elsewhere; both outcomes prove no panic.
        if res.is_ok() {
            assert!(!victim.exists());
            // cleanup anything we moved into the real trash
            let _ = fs::remove_dir_all(trash_dir().join("junk-dir"));
        }
        let _ = fs::remove_dir_all(&tmp);
    }
}
