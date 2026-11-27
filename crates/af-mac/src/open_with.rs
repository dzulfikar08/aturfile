//! Open With: the apps the system says can open some files, and opening
//! files in a chosen one.

use std::io;
use std::path::{Path, PathBuf};

/// An app that can open the files asked about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct OpenApp {
    /// Its display name, without ".app" (and with its folder when two
    /// installed apps share a name).
    pub name: String,
    pub path: PathBuf,
    /// The default app for every one of the files.
    pub is_default: bool,
}

/// Apps that can open all of `paths`, the shared default first and the rest
/// by name. Empty outside macOS.
pub fn apps_for(paths: &[PathBuf]) -> Vec<OpenApp> {
    let per_file: Vec<(Option<PathBuf>, Vec<PathBuf>)> =
        paths.iter().map(|p| imp::app_urls(p)).collect();
    merge(per_file, imp::display_name)
}

/// Open `paths` in the app at `app`.
pub fn open_with(paths: &[PathBuf], app: &Path) -> io::Result<()> {
    if paths.is_empty() {
        return Ok(());
    }
    imp::open_with(paths, app)
}

/// Keep the apps every file has, mark the default they share, sort, and
/// tell same-named apps apart.
fn merge(
    per_file: Vec<(Option<PathBuf>, Vec<PathBuf>)>,
    name: impl Fn(&Path) -> String,
) -> Vec<OpenApp> {
    let mut files = per_file.into_iter();
    let Some((first_default, mut common)) = files.next() else {
        return Vec::new();
    };
    let mut default = first_default;
    for (d, apps) in files {
        common.retain(|a| apps.contains(a));
        if default != d {
            default = None;
        }
    }
    let mut seen = Vec::new();
    common.retain(|a| {
        let fresh = !seen.contains(a);
        seen.push(a.clone());
        fresh
    });
    if let Some(d) = &default
        && !common.contains(d)
    {
        common.push(d.clone());
    }
    let mut out: Vec<OpenApp> = common
        .into_iter()
        .map(|path| OpenApp {
            name: name(&path),
            is_default: default.as_ref() == Some(&path),
            path,
        })
        .collect();
    let names: Vec<String> = out.iter().map(|a| a.name.clone()).collect();
    for app in &mut out {
        if names.iter().filter(|n| **n == app.name).count() > 1
            && let Some(dir) = app.path.parent().and_then(|p| p.file_name())
        {
            app.name = format!("{} ({})", app.name, dir.to_string_lossy());
        }
    }
    out.sort_by(|a, b| {
        b.is_default
            .cmp(&a.is_default)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    out
}

/// The name an app goes by: its file name without ".app".
fn plain_name(app: &Path) -> String {
    let name = app
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    name.strip_suffix(".app").map(str::to_owned).unwrap_or(name)
}

#[cfg(target_os = "macos")]
mod imp {
    use std::io;
    use std::path::{Path, PathBuf};

    use objc2::rc::Retained;
    use objc2_app_kit::{NSWorkspace, NSWorkspaceOpenConfiguration};
    use objc2_foundation::{NSArray, NSFileManager, NSString, NSURL};

    fn file_url(path: &Path) -> Retained<NSURL> {
        NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
    }

    fn url_path(url: &NSURL) -> Option<PathBuf> {
        url.path().map(|p| PathBuf::from(p.to_string()))
    }

    pub fn app_urls(path: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
        let ws = NSWorkspace::sharedWorkspace();
        let url = file_url(path);
        let default = ws
            .URLForApplicationToOpenURL(&url)
            .and_then(|u| url_path(&u));
        let all = ws
            .URLsForApplicationsToOpenURL(&url)
            .iter()
            .filter_map(|u| url_path(&u))
            .collect();
        (default, all)
    }

    pub fn display_name(app: &Path) -> String {
        let shown = NSFileManager::defaultManager()
            .displayNameAtPath(&NSString::from_str(&app.to_string_lossy()))
            .to_string();
        let name = shown.strip_suffix(".app").unwrap_or(&shown);
        if name.is_empty() {
            super::plain_name(app)
        } else {
            name.to_owned()
        }
    }

    pub fn open_with(paths: &[PathBuf], app: &Path) -> io::Result<()> {
        if !app.exists() {
            return Err(io::Error::new(io::ErrorKind::NotFound, "app not found"));
        }
        let urls: Vec<Retained<NSURL>> = paths.iter().map(|p| file_url(p)).collect();
        let urls = NSArray::from_retained_slice(&urls);
        let config = NSWorkspaceOpenConfiguration::configuration();
        NSWorkspace::sharedWorkspace()
            .openURLs_withApplicationAtURL_configuration_completionHandler(
                &urls,
                &file_url(app),
                &config,
                None,
            );
        Ok(())
    }
}

#[cfg(not(target_os = "macos"))]
mod imp {
    use std::io;
    use std::path::{Path, PathBuf};

    pub fn app_urls(_path: &Path) -> (Option<PathBuf>, Vec<PathBuf>) {
        (None, Vec::new())
    }

    pub fn display_name(app: &Path) -> String {
        super::plain_name(app)
    }

    pub fn open_with(_paths: &[PathBuf], _app: &Path) -> io::Result<()> {
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "Open With is only on macOS",
        ))
    }
}

#[cfg(test)]
mod tests {
    use std::path::{Path, PathBuf};

    use super::{merge, plain_name};

    fn p(s: &str) -> PathBuf {
        PathBuf::from(s)
    }

    #[test]
    fn keeps_shared_apps_default_first() {
        let preview = p("/System/Applications/Preview.app");
        let photos = p("/System/Applications/Photos.app");
        let pixel = p("/Applications/Pixelmator Pro.app");
        let one = (
            Some(preview.clone()),
            vec![pixel.clone(), preview.clone(), photos.clone()],
        );
        let two = (Some(preview.clone()), vec![preview.clone(), pixel.clone()]);
        let apps = merge(vec![one, two], plain_name);
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Preview", "Pixelmator Pro"]);
        assert!(apps[0].is_default && !apps[1].is_default);
    }

    #[test]
    fn mixed_defaults_mark_none() {
        let a = p("/Applications/A.app");
        let b = p("/Applications/B.app");
        let apps = merge(
            vec![
                (Some(a.clone()), vec![a.clone(), b.clone()]),
                (Some(b.clone()), vec![b.clone(), a.clone()]),
            ],
            plain_name,
        );
        assert!(apps.iter().all(|x| !x.is_default));
        assert_eq!(apps.len(), 2);
    }

    #[test]
    fn same_names_show_their_folder() {
        let apps = merge(
            vec![(
                None,
                vec![
                    p("/Applications/Xcode.app"),
                    p("/Users/me/Downloads/Xcode.app"),
                    p("/Applications/Xcode.app"),
                ],
            )],
            plain_name,
        );
        let names: Vec<&str> = apps.iter().map(|a| a.name.as_str()).collect();
        assert_eq!(names, ["Xcode (Applications)", "Xcode (Downloads)"]);
        assert!(merge(Vec::new(), plain_name).is_empty());
        assert_eq!(plain_name(Path::new("/Applications/Zed.app")), "Zed");
    }
}
