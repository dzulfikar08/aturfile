use std::path::{Path, PathBuf};

/// The user's home folder, the real one even inside the App Sandbox.
pub fn home() -> PathBuf {
    af_mac::real_home()
}

/// Per-user app data dir (the sandbox container's in App Store builds).
pub fn app_data_dir() -> PathBuf {
    af_mac::app_data_dir()
}

/// Trash destination for this user.
pub fn trash_dir() -> PathBuf {
    if cfg!(target_os = "macos") {
        home().join(".Trash")
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .unwrap_or_else(|| home().join(".local/share"))
            .join("Trash/files")
    }
}

/// Strip a `~` prefix against `home`, for display.
pub fn display_path(p: &Path, home: &Path) -> String {
    p.strip_prefix(home)
        .map(|rel| format!("~/{}", rel.display()))
        .unwrap_or_else(|_| p.display().to_string())
}
