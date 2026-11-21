//! macOS glue for aturfile, with plain fallbacks elsewhere:
//!
//! - [`real_home`]: the user's home folder even inside the App Sandbox, where
//!   `$HOME` points at the app's container.
//! - [`trash`]: Finder's Trash, so "Put Back" works and sandboxed builds can
//!   trash files the user granted.
//! - [`Grants`]: folders the user opened through the open panel, kept as
//!   security-scoped bookmarks so the sandboxed app can reach them next launch.

use std::io;
use std::path::{Path, PathBuf};

/// True inside the App Sandbox (Mac App Store builds).
pub fn is_sandboxed() -> bool {
    std::env::var_os("APP_SANDBOX_CONTAINER_ID").is_some()
}

/// The account's home folder from the user database, falling back to `$HOME`.
pub fn real_home() -> PathBuf {
    passwd_home()
        .or_else(|| std::env::var_os("HOME").map(PathBuf::from))
        .unwrap_or_else(|| PathBuf::from("/"))
}

/// Where the app keeps its own state: the container in the sandbox, the
/// usual per-user data folder otherwise.
pub fn app_data_dir() -> PathBuf {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_else(real_home);
    if cfg!(target_os = "macos") {
        home.join("Library/Application Support/aturfile")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home.join(".local/share"))
            .join("aturfile")
    }
}

#[cfg(unix)]
fn passwd_home() -> Option<PathBuf> {
    use std::ffi::CStr;
    use std::os::unix::ffi::OsStrExt;

    let mut buf = vec![0 as libc::c_char; 4096];
    // SAFETY: zeroed passwd is a valid out-parameter; getpwuid_r fills it with
    // pointers into `buf`, which outlives every read below.
    let mut pwd: libc::passwd = unsafe { std::mem::zeroed() };
    let mut result: *mut libc::passwd = std::ptr::null_mut();
    let rc = unsafe {
        libc::getpwuid_r(
            libc::getuid(),
            &mut pwd,
            buf.as_mut_ptr(),
            buf.len(),
            &mut result,
        )
    };
    if rc != 0 || result.is_null() || pwd.pw_dir.is_null() {
        return None;
    }
    let dir = unsafe { CStr::from_ptr(pwd.pw_dir) };
    let path = PathBuf::from(std::ffi::OsStr::from_bytes(dir.to_bytes()));
    (!path.as_os_str().is_empty()).then_some(path)
}

#[cfg(not(unix))]
fn passwd_home() -> Option<PathBuf> {
    None
}

// ---- Trash ------------------------------------------------------------------

/// Move `path` to the Trash. On macOS this is the Finder's Trash (Put Back
/// works); elsewhere the freedesktop Trash under `$XDG_DATA_HOME/Trash`.
pub fn trash(path: &Path) -> io::Result<()> {
    imp::trash(path)
}

// ---- Folder grants ------------------------------------------------------------

/// Folders the sandboxed app may read, kept across launches.
pub struct Grants {
    file: PathBuf,
    /// Paths currently being accessed (released on drop).
    active: Vec<imp::Access>,
}

impl Grants {
    /// Load saved grants and start accessing each. Outside the sandbox this is
    /// empty and everything is reachable anyway.
    pub fn restore() -> Self {
        let file = app_data_dir().join("grants.bin");
        let mut active = Vec::new();
        if is_sandboxed() {
            for blob in read_blobs(&file) {
                if let Some(access) = imp::resolve(&blob) {
                    active.push(access);
                }
            }
        }
        Self { file, active }
    }

    pub fn paths(&self) -> Vec<PathBuf> {
        self.active.iter().map(|a| a.path.clone()).collect()
    }

    /// Whether `path` is inside a granted folder (always true unsandboxed).
    pub fn covers(&self, path: &Path) -> bool {
        !is_sandboxed() || self.active.iter().any(|a| path.starts_with(&a.path))
    }

    /// Remember `path`, just chosen by the user in an open panel. Call it in
    /// the same run as the panel: the sandbox only lets this process bookmark
    /// it then.
    pub fn add(&mut self, path: &Path) -> io::Result<()> {
        if !is_sandboxed() || self.covers(path) {
            return Ok(());
        }
        let blob = imp::bookmark(path)?;
        if let Some(access) = imp::resolve(&blob) {
            self.active.push(access);
        }
        let mut blobs = read_blobs(&self.file);
        blobs.push(blob);
        write_blobs(&self.file, &blobs)
    }
}

/// Length-prefixed blobs: `u32 LE length` then the bytes, repeated.
fn read_blobs(file: &Path) -> Vec<Vec<u8>> {
    let Ok(data) = std::fs::read(file) else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let mut rest = data.as_slice();
    while rest.len() >= 4 {
        let len = u32::from_le_bytes([rest[0], rest[1], rest[2], rest[3]]) as usize;
        rest = &rest[4..];
        if rest.len() < len {
            break;
        }
        out.push(rest[..len].to_vec());
        rest = &rest[len..];
    }
    out
}

fn write_blobs(file: &Path, blobs: &[Vec<u8>]) -> io::Result<()> {
    if let Some(dir) = file.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut data = Vec::new();
    for b in blobs {
        data.extend_from_slice(&(b.len() as u32).to_le_bytes());
        data.extend_from_slice(b);
    }
    let tmp = file.with_extension("tmp");
    std::fs::write(&tmp, data)?;
    std::fs::rename(tmp, file)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io;
    use std::path::{Path, PathBuf};

    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_foundation::{
        NSData, NSFileManager, NSString, NSURL, NSURLBookmarkCreationOptions,
        NSURLBookmarkResolutionOptions,
    };

    pub struct Access {
        pub path: PathBuf,
        url: Retained<NSURL>,
    }

    impl Drop for Access {
        fn drop(&mut self) {
            // SAFETY: balanced with the successful start in `resolve`.
            unsafe { self.url.stopAccessingSecurityScopedResource() };
        }
    }

    fn file_url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    fn ns_error(e: &objc2_foundation::NSError) -> io::Error {
        io::Error::other(e.localizedDescription().to_string())
    }

    pub fn trash(path: &Path) -> io::Result<()> {
        let url = file_url(path);
        NSFileManager::defaultManager()
            .trashItemAtURL_resultingItemURL_error(&url, None)
            .map_err(|e| ns_error(&e))
    }

    pub fn bookmark(path: &Path) -> io::Result<Vec<u8>> {
        let data = file_url(path)
            .bookmarkDataWithOptions_includingResourceValuesForKeys_relativeToURL_error(
                NSURLBookmarkCreationOptions::WithSecurityScope,
                None,
                None,
            )
            .map_err(|e| ns_error(&e))?;
        Ok(data.to_vec())
    }

    pub fn resolve(blob: &[u8]) -> Option<Access> {
        let data = NSData::with_bytes(blob);
        let mut stale = Bool::NO;
        // SAFETY: `stale` is a valid pointer for the duration of the call.
        let url = unsafe {
            NSURL::URLByResolvingBookmarkData_options_relativeToURL_bookmarkDataIsStale_error(
                &data,
                NSURLBookmarkResolutionOptions::WithSecurityScope,
                None,
                &mut stale,
            )
        }
        .ok()?;
        // SAFETY: plain Objective-C call; balanced by `Access::drop`.
        if !unsafe { url.startAccessingSecurityScopedResource() } {
            return None;
        }
        let path = PathBuf::from(url.path()?.to_string());
        Some(Access { path, url })
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::io;
    use std::path::{Path, PathBuf};

    pub struct Access {
        pub path: PathBuf,
    }

    pub fn bookmark(_path: &Path) -> io::Result<Vec<u8>> {
        Err(io::Error::other("folder grants exist only on macOS"))
    }

    pub fn resolve(_blob: &[u8]) -> Option<Access> {
        None
    }

    /// freedesktop.org Trash: the file plus a `.trashinfo` with its origin, so
    /// file managers can restore it.
    pub fn trash(path: &Path) -> io::Result<()> {
        let base = std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| super::real_home().join(".local/share"))
            .join("Trash");
        let files = base.join("files");
        let info = base.join("info");
        std::fs::create_dir_all(&files)?;
        std::fs::create_dir_all(&info)?;
        let name = path
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "item".into());
        let mut candidate = name.clone();
        let mut n = 1;
        while files.join(&candidate).symlink_metadata().is_ok()
            || info.join(format!("{candidate}.trashinfo")).exists()
        {
            n += 1;
            candidate = format!("{name}.{n}");
        }
        let abs = std::path::absolute(path)?;
        std::fs::write(
            info.join(format!("{candidate}.trashinfo")),
            format!(
                "[Trash Info]\nPath={}\nDeletionDate={}\n",
                abs.display(),
                deletion_date()
            ),
        )?;
        let dest = files.join(&candidate);
        if let Err(e) = std::fs::rename(path, &dest) {
            let _ = std::fs::remove_file(info.join(format!("{candidate}.trashinfo")));
            return Err(e);
        }
        Ok(())
    }

    fn deletion_date() -> String {
        // Local time isn't available without a date crate; UTC is accepted
        // by every implementation we target.
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_secs() as i64)
            .unwrap_or(0);
        let (y, m, d) = civil(secs.div_euclid(86_400));
        let t = secs.rem_euclid(86_400);
        format!(
            "{y:04}-{m:02}-{d:02}T{:02}:{:02}:{:02}",
            t / 3600,
            t / 60 % 60,
            t % 60
        )
    }

    fn civil(days: i64) -> (i64, i64, i64) {
        let z = days + 719_468;
        let era = z.div_euclid(146_097);
        let doe = z - era * 146_097;
        let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
        let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
        let mp = (5 * doy + 2) / 153;
        let d = doy - (153 * mp + 2) / 5 + 1;
        let m = if mp < 10 { mp + 3 } else { mp - 9 };
        (yoe + era * 400 + i64::from(m <= 2), m, d)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blobs_round_trip() {
        let dir = tempfile::tempdir().unwrap();
        let file = dir.path().join("g.bin");
        write_blobs(&file, &[vec![1, 2, 3], vec![], vec![9; 300]]).unwrap();
        assert_eq!(read_blobs(&file), vec![vec![1, 2, 3], vec![], vec![9; 300]]);
    }

    #[test]
    fn home_is_absolute() {
        assert!(real_home().is_absolute());
    }

    #[cfg(not(target_os = "macos"))]
    #[test]
    fn freedesktop_trash_records_origin() {
        let dir = tempfile::tempdir().unwrap();
        let data = dir.path().join("data");
        // SAFETY: tests in this crate don't read XDG_DATA_HOME concurrently.
        unsafe { std::env::set_var("XDG_DATA_HOME", &data) };
        let victim = dir.path().join("old.log");
        std::fs::write(&victim, "x").unwrap();
        trash(&victim).unwrap();
        assert!(!victim.exists());
        assert!(data.join("Trash/files/old.log").exists());
        let info = std::fs::read_to_string(data.join("Trash/info/old.log.trashinfo")).unwrap();
        assert!(info.contains(&format!("Path={}", victim.display())));
    }
}
