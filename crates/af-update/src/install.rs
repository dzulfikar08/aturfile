//! Staging a downloaded build next to aturfile.app, and swapping it in.
//!
//! Staging does everything slow or fallible: download, size and SHA-256
//! check, unpack, code-signature and notarisation checks, and a `--version`
//! run of the new binary. The swap is two renames on one filesystem: the old
//! bundle moves aside, the new one moves in, and the old one is put back if
//! that fails. The running process keeps its (already mapped) old binary.

use std::fs;
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

use serde::{Deserialize, Serialize};

use crate::http::Http;
use crate::manifest::{Asset, Manifest};
use crate::verify::Verifier;
use crate::{Error, InstallKind};

/// Scratch folder beside the bundle (same filesystem, so the swap is a rename).
pub const WORK_DIR: &str = ".aturfile-update";

/// A verified build waiting to be swapped in (saved across runs).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Staged {
    pub version: String,
    pub notes_url: String,
    /// The installed bundle the update replaces.
    pub bundle: PathBuf,
    /// The verified new aturfile.app, inside `work_dir`.
    pub new_app: PathBuf,
    /// Scratch space removed after the swap (or when the update is dropped).
    pub work_dir: Option<PathBuf>,
}

/// Download `asset` to `dest`, checking its size and SHA-256. `dest` is
/// removed on any failure.
pub fn download_verified(
    http: &Http,
    asset: &Asset,
    dest: &Path,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<(), Error> {
    let result = (|| {
        let file = create(dest)?;
        let mut out = HashingWriter {
            inner: io::BufWriter::new(file),
            digest: ring::digest::Context::new(&ring::digest::SHA256),
        };
        let total = asset.size;
        let n = http.download(&asset.url, &mut out, total, &mut |done| {
            progress(done, total)
        })?;
        out.inner.flush()?;
        out.inner.get_ref().sync_all()?;
        check_digest(n, out.digest.finish().as_ref(), asset)
    })();
    if result.is_err() {
        let _ = fs::remove_file(dest);
    }
    result
}

/// The downloaded byte count and digest match the manifest.
pub fn check_digest(size: u64, digest: &[u8], asset: &Asset) -> Result<(), Error> {
    if size != asset.size {
        return Err(Error::Checksum(format!(
            "got {size} bytes, the manifest says {}",
            asset.size
        )));
    }
    let hex: String = digest.iter().map(|b| format!("{b:02x}")).collect();
    if hex != asset.sha256 {
        return Err(Error::Checksum(
            "SHA-256 doesn't match the signed manifest".into(),
        ));
    }
    Ok(())
}

struct HashingWriter<W> {
    inner: W,
    digest: ring::digest::Context,
}

impl<W: Write> Write for HashingWriter<W> {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let n = self.inner.write(buf)?;
        self.digest.update(&buf[..n]);
        Ok(n)
    }
    fn flush(&mut self) -> io::Result<()> {
        self.inner.flush()
    }
}

fn not_writable(e: &io::Error) -> bool {
    matches!(
        e.kind(),
        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
    )
}

fn io_err(path: &Path, e: io::Error) -> Error {
    Error::Io(format!("{}: {e}", path.display()))
}

fn create(path: &Path) -> Result<fs::File, Error> {
    fs::File::create(path).map_err(|e| {
        if not_writable(&e) {
            Error::NotWritable(path.parent().unwrap_or(path).to_path_buf())
        } else {
            io_err(path, e)
        }
    })
}

/// A fresh, empty `path`.
fn create_dir(path: &Path) -> Result<(), Error> {
    if path.exists() {
        fs::remove_dir_all(path).map_err(|e| io_err(path, e))?;
    }
    fs::create_dir_all(path).map_err(|e| {
        if not_writable(&e) {
            Error::NotWritable(path.parent().unwrap_or(path).to_path_buf())
        } else {
            io_err(path, e)
        }
    })
}

/// Renames into `dir` will work: aturfile can create files there.
fn check_writable(dir: &Path) -> Result<(), Error> {
    tempfile::NamedTempFile::new_in(dir).map(drop).map_err(|e| {
        if not_writable(&e) {
            Error::NotWritable(dir.to_path_buf())
        } else {
            io_err(dir, e)
        }
    })
}

/// The new build starts and says it is `version` (`aturfile 1.1.0`).
fn probe_version(verifier: &dyn Verifier, program: &Path, version: &str) -> Result<(), Error> {
    let out = verifier.probe(program, &["--version"])?;
    let reported = out.lines().next().unwrap_or_default().trim();
    if reported.split_whitespace().nth(1) != Some(version) {
        return Err(Error::Verify(format!(
            "the new build reports {reported:?}, expected aturfile {version}"
        )));
    }
    Ok(())
}

/// Download and check `asset` to replace `bundle`, leaving it ready to swap
/// in (see [`apply`]).
pub fn stage(
    manifest: &Manifest,
    asset: &Asset,
    bundle: &Path,
    http: &Http,
    verifier: &dyn Verifier,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Staged, Error> {
    let parent = bundle.parent().ok_or(Error::Unmanaged)?;
    let work = parent.join(WORK_DIR);
    let result = (|| {
        check_writable(parent)?;
        create_dir(&work)?;
        let archive = work.join("download.tar.gz");
        download_verified(http, asset, &archive, progress)?;
        let root = unpack(&archive, &work.join("x"))?;
        let _ = fs::remove_file(&archive);
        let app = root.join("aturfile.app");
        verifier.check_app(&app)?;
        probe_version(
            verifier,
            &app.join("Contents/MacOS/aturfile"),
            &manifest.version,
        )?;
        Ok(app)
    })();
    match result {
        Ok(new_app) => Ok(Staged {
            version: manifest.version.clone(),
            notes_url: manifest.notes_url.clone(),
            bundle: bundle.to_path_buf(),
            new_app,
            work_dir: Some(work),
        }),
        Err(e) => {
            let _ = fs::remove_dir_all(&work);
            Err(e)
        }
    }
}

fn holds_app(dir: &Path) -> bool {
    dir.join("aturfile.app/Contents/MacOS/aturfile").is_file()
}

/// Unpack a .tar.gz with the system `tar` (bsdtar on macOS, which keeps the
/// app's extended attributes) and return the folder holding aturfile.app:
/// `into` itself or the archive's top folder.
fn unpack(archive: &Path, into: &Path) -> Result<PathBuf, Error> {
    fs::create_dir_all(into).map_err(|e| io_err(into, e))?;
    let out = Command::new("tar")
        .arg("-xzf")
        .arg(archive)
        .arg("-C")
        .arg(into)
        .output()
        .map_err(|e| Error::Io(format!("can't run tar: {e}")))?;
    if !out.status.success() {
        return Err(Error::Io(format!(
            "unpacking the update failed: {}",
            String::from_utf8_lossy(&out.stderr).trim()
        )));
    }
    if holds_app(into) {
        return Ok(into.to_path_buf());
    }
    fs::read_dir(into)
        .map_err(|e| io_err(into, e))?
        .filter_map(Result::ok)
        .map(|e| e.path())
        .find(|p| p.is_dir() && holds_app(p))
        .ok_or_else(|| Error::Io("the update archive doesn't hold aturfile.app".into()))
}

// ── swapping ─────────────────────────────────────────────────────────────

fn backup_path(target: &Path) -> PathBuf {
    let name = target.file_name().unwrap_or_default().to_string_lossy();
    target.with_file_name(format!(".{name}.aturfile-old"))
}

fn remove_any(path: &Path) -> io::Result<()> {
    match fs::symlink_metadata(path) {
        Ok(m) if m.is_dir() => fs::remove_dir_all(path),
        Ok(_) => fs::remove_file(path),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
        Err(e) => Err(e),
    }
}

/// Put `new` at `target`: the old bundle moves aside first and comes back if
/// the second rename fails, so `target` is never left half-replaced.
pub fn swap(new: &Path, target: &Path) -> Result<(), Error> {
    if fs::symlink_metadata(new).is_err() {
        return Err(Error::Io(format!(
            "the downloaded update is gone ({})",
            new.display()
        )));
    }
    let wrap = |e: io::Error| {
        if not_writable(&e) {
            Error::NotWritable(target.parent().unwrap_or(target).to_path_buf())
        } else {
            io_err(target, e)
        }
    };
    let backup = backup_path(target);
    remove_any(&backup).map_err(wrap)?;
    let had_old = fs::symlink_metadata(target).is_ok();
    if had_old {
        fs::rename(target, &backup).map_err(wrap)?;
    }
    if let Err(e) = fs::rename(new, target) {
        if had_old {
            let _ = fs::rename(&backup, target);
        }
        return Err(wrap(e));
    }
    let _ = remove_any(&backup);
    Ok(())
}

/// At start-up: remove what an earlier update left beside the bundle (a
/// failed download's scratch folder, a backup), keeping `staged`'s files.
pub fn clean_leftovers(kind: &InstallKind, staged: Option<&Staged>) {
    let InstallKind::MacApp { bundle } = kind else {
        return;
    };
    let Some(parent) = bundle.parent() else {
        return;
    };
    let work = parent.join(WORK_DIR);
    let keep = staged.is_some_and(|s| s.work_dir.as_deref() == Some(work.as_path()));
    if !keep && work.exists() {
        let _ = fs::remove_dir_all(&work);
    }
    let _ = remove_any(&backup_path(bundle));
}

/// Swap the staged build in, then drop the scratch space.
pub fn apply(staged: &Staged) -> Result<(), Error> {
    swap(&staged.new_app, &staged.bundle)?;
    discard(staged);
    Ok(())
}

/// [`apply`], then start the new build ("Restart to Update"). A failed
/// relaunch is not an error: the new build is installed and starts next time.
pub fn apply_and_relaunch(staged: &Staged) -> Result<(), Error> {
    apply(staged)?;
    if let Err(e) = relaunch(&staged.bundle) {
        eprintln!("aturfile: installed, but relaunching failed: {e}");
    }
    Ok(())
}

/// Remove whatever is left of a staged update.
pub fn discard(staged: &Staged) {
    let _ = remove_any(&staged.new_app);
    if let Some(work) = &staged.work_dir {
        let _ = fs::remove_dir_all(work);
    }
}

/// Start `bundle` with `open -n`, detached from this process, so it outlives
/// this one quitting.
pub fn relaunch(bundle: &Path) -> io::Result<()> {
    let mut cmd = Command::new("/usr/bin/open");
    cmd.arg("-n")
        .arg(bundle)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null());
    #[cfg(unix)]
    {
        use std::os::unix::process::CommandExt as _;
        cmd.process_group(0);
    }
    cmd.spawn().map(drop)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, text: &str) {
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(path, text).unwrap();
    }

    fn read(path: &Path) -> String {
        fs::read_to_string(path).unwrap()
    }

    #[test]
    fn digest_mismatch() {
        let digest = ring::digest::digest(&ring::digest::SHA256, b"hello");
        let hex: String = digest.as_ref().iter().map(|b| format!("{b:02x}")).collect();
        let asset = Asset {
            url: "https://x/y".into(),
            sha256: hex,
            size: 5,
        };
        check_digest(5, digest.as_ref(), &asset).unwrap();
        assert!(matches!(
            check_digest(6, digest.as_ref(), &asset),
            Err(Error::Checksum(_))
        ));
        let other = ring::digest::digest(&ring::digest::SHA256, b"hellO");
        assert!(matches!(
            check_digest(5, other.as_ref(), &asset),
            Err(Error::Checksum(_))
        ));
    }

    #[test]
    fn swaps_the_bundle() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        write(&p.join("aturfile.app/Contents/MacOS/aturfile"), "old");
        write(&p.join("aturfile.app/Contents/Old-only"), "x");
        write(&p.join("stage/aturfile.app/Contents/MacOS/aturfile"), "new");
        swap(&p.join("stage/aturfile.app"), &p.join("aturfile.app")).unwrap();
        assert_eq!(read(&p.join("aturfile.app/Contents/MacOS/aturfile")), "new");
        assert!(!p.join("aturfile.app/Contents/Old-only").exists());
        assert!(!p.join(".aturfile.app.aturfile-old").exists());
        assert!(!p.join("stage/aturfile.app").exists());
    }

    #[test]
    fn missing_staged_app_changes_nothing() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        write(&p.join("aturfile.app/Contents/MacOS/aturfile"), "old");
        assert!(swap(&p.join("gone/aturfile.app"), &p.join("aturfile.app")).is_err());
        assert_eq!(read(&p.join("aturfile.app/Contents/MacOS/aturfile")), "old");
    }

    #[test]
    fn apply_and_leftovers() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        let bundle = p.join("aturfile.app");
        let work = p.join(WORK_DIR);
        write(&bundle.join("Contents/MacOS/aturfile"), "old");
        write(&work.join("x/aturfile.app/Contents/MacOS/aturfile"), "new");
        let staged = Staged {
            version: "9.9.9".into(),
            notes_url: String::new(),
            bundle: bundle.clone(),
            new_app: work.join("x/aturfile.app"),
            work_dir: Some(work.clone()),
        };
        let kind = InstallKind::MacApp {
            bundle: bundle.clone(),
        };
        // A staged update's files stay at start-up; a stray backup goes.
        write(&p.join(".aturfile.app.aturfile-old/x"), "stale");
        clean_leftovers(&kind, Some(&staged));
        assert!(staged.new_app.exists());
        assert!(!p.join(".aturfile.app.aturfile-old").exists());
        apply(&staged).unwrap();
        assert_eq!(read(&bundle.join("Contents/MacOS/aturfile")), "new");
        assert!(!work.exists(), "scratch space removed");
        // Without a staged update, a leftover scratch folder is removed.
        write(&work.join("download.tar.gz"), "partial");
        clean_leftovers(&kind, None);
        assert!(!work.exists());
    }

    #[test]
    fn unpacks_the_top_folder() {
        let d = tempfile::tempdir().unwrap();
        let p = d.path();
        write(
            &p.join("src/aturfile-1.1.0-universal-macos/aturfile.app/Contents/MacOS/aturfile"),
            "bin",
        );
        let archive = p.join("a.tar.gz");
        let ok = Command::new("tar")
            .arg("-czf")
            .arg(&archive)
            .arg("-C")
            .arg(p.join("src"))
            .arg("aturfile-1.1.0-universal-macos")
            .status()
            .unwrap()
            .success();
        assert!(ok);
        let root = unpack(&archive, &p.join("x")).unwrap();
        assert!(root.ends_with("aturfile-1.1.0-universal-macos"));
        assert!(holds_app(&root));
    }

    #[cfg(unix)]
    #[test]
    fn read_only_dir_is_not_writable() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = tempfile::tempdir().unwrap();
        let ro = d.path().join("ro");
        fs::create_dir(&ro).unwrap();
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o555)).unwrap();
        // Root ignores permissions; nothing to test then.
        if tempfile::NamedTempFile::new_in(&ro).is_err() {
            assert!(matches!(check_writable(&ro), Err(Error::NotWritable(_))));
            assert!(matches!(create(&ro.join("f")), Err(Error::NotWritable(_))));
        }
        fs::set_permissions(&ro, fs::Permissions::from_mode(0o755)).unwrap();
    }
}
