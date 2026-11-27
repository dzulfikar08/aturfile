//! In-app updates for the Developer ID build (no GPUI). The Mac App Store
//! build never uses this: the App Store updates it.
//!
//! ```text
//! check   GET update-stable.json and .sig → Ed25519 verify → newer than this build?
//! stage   download beside aturfile.app → size + SHA-256 → unpack → codesign,
//!         spctl → run the new binary's --version
//! apply   swap the new aturfile.app in (the old one is kept until the swap
//!         has worked), on "Restart to Update" or at quit; then relaunch
//! ```
//!
//! A check sends nothing but the User-Agent (`aturfile/<version> (macos; <arch>)`).
//! Copies not running from an app bundle (`cargo run`) only get a notice.

pub mod http;
pub mod install;
pub mod manifest;
pub mod state;
pub mod verify;

use std::fmt;
use std::path::{Path, PathBuf};

pub use install::{Staged, apply, apply_and_relaunch, clean_leftovers, discard, relaunch};
pub use manifest::{Asset, Manifest};
pub use semver::Version;
pub use state::State;

use http::Http;
use verify::Verifier;

/// Where releases publish the manifest (and `<url>.sig`).
pub const MANIFEST_URL: &str =
    "https://github.com/dzulfikar08/aturfile/releases/latest/download/update-stable.json";

/// The manifest's key for the macOS download.
pub const MAC_ASSET: &str = "macos-universal-app";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Error {
    Network(String),
    /// The manifest's signature doesn't verify with aturfile's key.
    Signature,
    Manifest(String),
    /// A download doesn't match the size or SHA-256 the manifest promised.
    Checksum(String),
    /// aturfile can't write where it is installed; download it by hand.
    NotWritable(PathBuf),
    /// The new build failed a signature, notarisation or start-up check.
    Verify(String),
    /// Not running from an app bundle (a source build).
    Unmanaged,
    Io(String),
}

impl fmt::Display for Error {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Network(e) => write!(f, "couldn't reach the update server ({e})"),
            Self::Signature => f.write_str("the update manifest's signature is not valid"),
            Self::Manifest(e) => write!(f, "the update manifest is invalid ({e})"),
            Self::Checksum(e) => write!(f, "the download is corrupt ({e})"),
            Self::NotWritable(dir) => write!(f, "aturfile can't write to {}", dir.display()),
            Self::Verify(e) => f.write_str(e),
            Self::Unmanaged => f.write_str("this copy of aturfile is not an app bundle"),
            Self::Io(e) => f.write_str(e),
        }
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

/// This build's version.
pub fn current_version() -> Version {
    Version::parse(env!("CARGO_PKG_VERSION")).expect("the crate version is semver")
}

/// The version updates are compared against: [`current_version`], unless
/// `AF_UPDATE_CURRENT` (e.g. `0.1.0`) pretends to be older. Debug builds
/// only: in a release it would let a replayed, older signed manifest
/// downgrade the app.
pub fn running_version() -> Version {
    if !cfg!(debug_assertions) {
        return current_version();
    }
    std::env::var("AF_UPDATE_CURRENT")
        .ok()
        .and_then(|v| Version::parse(v.trim()).ok())
        .unwrap_or_else(current_version)
}

/// [`MANIFEST_URL`], or `AF_UPDATE_URL` (debug builds, for a local server).
pub fn manifest_url() -> String {
    match std::env::var("AF_UPDATE_URL") {
        Ok(url) if cfg!(debug_assertions) && !url.trim().is_empty() => url.trim().to_owned(),
        _ => MANIFEST_URL.to_owned(),
    }
}

/// How this copy is installed.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub enum InstallKind {
    /// Running from `bundle` (`…/aturfile.app`), which can be replaced.
    MacApp { bundle: PathBuf },
    /// Anything else (`cargo run`, Linux): updates are only announced.
    Unmanaged,
}

impl InstallKind {
    /// For this process.
    pub fn detect() -> Self {
        if !cfg!(target_os = "macos") {
            return Self::Unmanaged;
        }
        match std::env::current_exe().and_then(|p| p.canonicalize()) {
            Ok(exe) => Self::from_exe(&exe),
            Err(_) => Self::Unmanaged,
        }
    }

    /// `X.app/Contents/MacOS/aturfile` → `MacApp { X.app }`.
    pub fn from_exe(exe: &Path) -> Self {
        let macos = exe.parent();
        let contents = macos.and_then(Path::parent);
        let bundle = contents.and_then(Path::parent);
        match (macos, contents, bundle) {
            (Some(m), Some(c), Some(b))
                if m.file_name() == Some("MacOS".as_ref())
                    && c.file_name() == Some("Contents".as_ref())
                    && b.extension() == Some("app".as_ref()) =>
            {
                Self::MacApp {
                    bundle: b.to_path_buf(),
                }
            }
            _ => Self::Unmanaged,
        }
    }
}

/// What a check works from.
#[derive(Debug, Clone)]
pub struct Config {
    pub manifest_url: String,
    pub public_key: [u8; 32],
    pub current: Version,
    pub kind: InstallKind,
}

impl Config {
    pub fn for_this_build() -> Self {
        Self {
            manifest_url: manifest_url(),
            public_key: manifest::decode_public_key(manifest::PUBLIC_KEY)
                .expect("the embedded update key is valid"),
            current: running_version(),
            kind: InstallKind::detect(),
        }
    }
}

/// A newer build than this one.
#[derive(Debug, Clone)]
pub struct Available {
    pub manifest: Manifest,
    pub version: Version,
    /// The download; `None`: tell the user, don't install (not an app bundle,
    /// or no macOS build in the manifest).
    pub asset: Option<Asset>,
}

/// Fetch and verify the manifest; `None` when this build is current.
pub fn check(cfg: &Config, http: &Http) -> Result<Option<Available>, Error> {
    let bytes = http.get(&cfg.manifest_url, manifest::MAX_MANIFEST)?;
    let sig = http.get(&format!("{}.sig", cfg.manifest_url), 1024)?;
    evaluate(cfg, &bytes, &sig)
}

/// [`check`] after the fetch.
pub fn evaluate(cfg: &Config, bytes: &[u8], sig: &[u8]) -> Result<Option<Available>, Error> {
    let manifest = manifest::verify(bytes, sig, &cfg.public_key)?;
    let version = manifest.version()?;
    if version <= cfg.current {
        return Ok(None);
    }
    let asset = match cfg.kind {
        InstallKind::MacApp { .. } => manifest.assets.get(MAC_ASSET).cloned(),
        InstallKind::Unmanaged => None,
    };
    Ok(Some(Available {
        manifest,
        version,
        asset,
    }))
}

/// Download and verify `available`; see [`install::stage`].
pub fn stage(
    cfg: &Config,
    available: &Available,
    http: &Http,
    verifier: &dyn Verifier,
    progress: &mut dyn FnMut(u64, u64),
) -> Result<Staged, Error> {
    let asset = available.asset.as_ref().ok_or(Error::Unmanaged)?;
    let InstallKind::MacApp { bundle } = &cfg.kind else {
        return Err(Error::Unmanaged);
    };
    install::stage(&available.manifest, asset, bundle, http, verifier, progress)
}

/// A staged update is still worth applying: newer than this build, and its
/// files are still there.
pub fn staged_is_current(staged: &Staged, current: &Version) -> bool {
    Version::parse(&staged.version).is_ok_and(|v| &v > current)
        && std::fs::symlink_metadata(&staged.new_app).is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::manifest::tests::{keypair, public, sign};

    fn cfg(kind: InstallKind, current: &str) -> Config {
        Config {
            manifest_url: String::new(),
            public_key: public(&keypair(1)),
            current: Version::parse(current).unwrap(),
            kind,
        }
    }

    fn manifest(version: &str) -> Vec<u8> {
        format!(
            r#"{{ "channel": "stable", "version": "{version}",
  "date": "2026-11-27T00:00:00Z", "notes_url": "https://example.com/notes",
  "assets": {{ "macos-universal-app": {{ "url": "https://example.com/a.tar.gz", "sha256": "{}", "size": 10 }} }} }}"#,
            "ab".repeat(32)
        )
        .into_bytes()
    }

    #[test]
    fn newer_only() {
        let mac = InstallKind::MacApp {
            bundle: "/Applications/aturfile.app".into(),
        };
        let m = manifest("1.1.0");
        let sig = sign(&keypair(1), &m);
        let run = |current: &str| evaluate(&cfg(mac.clone(), current), &m, &sig).unwrap();
        let a = run("1.0.0").expect("newer");
        assert_eq!(a.version.to_string(), "1.1.0");
        assert_eq!(
            a.asset.map(|a| a.url).as_deref(),
            Some("https://example.com/a.tar.gz")
        );
        assert!(run("1.1.0").is_none(), "same version");
        assert!(run("2.0.0").is_none(), "never a downgrade");
        assert!(run("1.1.0-rc.1").is_some(), "release after its rc");
    }

    #[test]
    fn unmanaged_is_told_not_installed() {
        let m = manifest("1.1.0");
        let sig = sign(&keypair(1), &m);
        let a = evaluate(&cfg(InstallKind::Unmanaged, "1.0.0"), &m, &sig)
            .unwrap()
            .unwrap();
        assert!(a.asset.is_none());
    }

    #[test]
    fn bundle_detection() {
        assert_eq!(
            InstallKind::from_exe(Path::new(
                "/Applications/aturfile.app/Contents/MacOS/aturfile"
            )),
            InstallKind::MacApp {
                bundle: "/Applications/aturfile.app".into()
            }
        );
        assert_eq!(
            InstallKind::from_exe(Path::new("/src/aturfile/target/release/aturfile")),
            InstallKind::Unmanaged
        );
        assert_eq!(
            InstallKind::from_exe(Path::new("/x/aturfile/Contents/MacOS/aturfile")),
            InstallKind::Unmanaged
        );
    }

    #[test]
    fn versions() {
        assert_eq!(current_version().to_string(), env!("CARGO_PKG_VERSION"));
    }
}
