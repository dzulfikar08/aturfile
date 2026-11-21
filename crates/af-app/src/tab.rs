//! One tab: a location with its own history, view, listing, filter and
//! selection. Plain data; the workspace drives loading and rendering.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use af_fs::{Entry, Selection, SortKey};
use gpui_kit::{SharedString, Task, UniformListScrollHandle};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Page {
    Files,
    Cleanup,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    #[default]
    List,
    Columns,
    Icons,
}

impl ViewMode {
    pub fn parse(s: &str) -> Self {
        match s.to_ascii_lowercase().as_str() {
            "columns" => Self::Columns,
            "icons" => Self::Icons,
            _ => Self::List,
        }
    }

    pub fn as_str(self) -> &'static str {
        match self {
            Self::List => "list",
            Self::Columns => "columns",
            Self::Icons => "icons",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::List => "List",
            Self::Columns => "Columns",
            Self::Icons => "Icons",
        }
    }
}

/// A folder listed for the Miller columns (ancestors and the folder under
/// the cursor), sorted folders first by name.
pub struct Listing {
    pub entries: Vec<Entry>,
    pub error: Option<SharedString>,
}

pub struct Tab {
    pub id: u64,
    pub page: Page,
    pub view: ViewMode,
    pub cwd: PathBuf,
    /// Every entry of `cwd`, sorted; hidden ones included.
    pub entries: Vec<Entry>,
    /// Indexes into `entries` after the hidden and text filters.
    pub visible: Vec<usize>,
    pub selection: Selection,
    pub query: String,
    pub sort: SortKey,
    pub ascending: bool,
    pub back: Vec<PathBuf>,
    pub forward: Vec<PathBuf>,
    pub error: Option<SharedString>,
    pub loaded: bool,
    pub load_ms: u128,
    pub scroll: UniformListScrollHandle,
    pub load_task: Option<Task<()>>,
    /// Miller columns: listings by folder, and their loads in flight.
    pub listings: HashMap<PathBuf, Listing>,
    pub listing_tasks: HashMap<PathBuf, Task<()>>,
    /// Icons per row in the icon view, from the last layout.
    pub icon_cols: usize,
}

impl Tab {
    pub fn new(id: u64, cwd: PathBuf, view: ViewMode) -> Self {
        Self {
            id,
            page: Page::Files,
            view,
            cwd,
            entries: Vec::new(),
            visible: Vec::new(),
            selection: Selection::new(),
            query: String::new(),
            sort: SortKey::Name,
            ascending: true,
            back: Vec::new(),
            forward: Vec::new(),
            error: None,
            loaded: false,
            load_ms: 0,
            scroll: UniformListScrollHandle::new(),
            load_task: None,
            listings: HashMap::new(),
            listing_tasks: HashMap::new(),
            icon_cols: 1,
        }
    }

    pub fn title(&self) -> String {
        match self.page {
            Page::Cleanup => "Clean Up".into(),
            Page::Files => folder_name(&self.cwd),
        }
    }

    /// The entry under the cursor (keyboard focus, previewed).
    pub fn current(&self) -> Option<&Entry> {
        self.selection.cursor().and_then(|ix| self.entry_at(ix))
    }

    pub fn entry_at(&self, row: usize) -> Option<&Entry> {
        // `visible` may still index an older, longer listing: never panic.
        self.visible.get(row).and_then(|&i| self.entries.get(i))
    }

    /// Selected entries in view order.
    pub fn selected(&self) -> Vec<&Entry> {
        self.selection
            .rows()
            .filter_map(|row| self.entry_at(row))
            .collect()
    }

    pub fn selected_paths(&self) -> Vec<PathBuf> {
        self.selected()
            .into_iter()
            .map(|e| e.path.clone())
            .collect()
    }

    /// Re-apply the hidden and text filters, keeping the same items
    /// selected where they are still shown.
    pub fn refilter(&mut self, show_hidden: bool) {
        let keep = self.selected_paths();
        let cursor = self.current().map(|e| e.path.clone());
        self.refilter_keeping(show_hidden, &keep, cursor.as_deref());
    }

    /// Replace the listing (a load finished), keeping the selection by path.
    /// The selection is read before the old entries go, since `visible`
    /// indexes them.
    pub fn replace_entries(&mut self, entries: Vec<Entry>, show_hidden: bool) {
        let keep = self.selected_paths();
        let cursor = self.current().map(|e| e.path.clone());
        self.entries = entries;
        self.visible.clear();
        self.sort_entries();
        self.refilter_keeping(show_hidden, &keep, cursor.as_deref());
    }

    fn refilter_keeping(&mut self, show_hidden: bool, keep: &[PathBuf], cursor: Option<&Path>) {
        let shown: Vec<usize> = (0..self.entries.len())
            .filter(|&i| show_hidden || !self.entries[i].hidden)
            .collect();
        let subset: Vec<Entry> = shown.iter().map(|&i| self.entries[i].clone()).collect();
        self.visible = af_fs::filter(&subset, &self.query)
            .into_iter()
            .map(|j| shown[j])
            .collect();
        self.reselect(keep, cursor);
    }

    /// Select `paths` (where shown), the cursor on `cursor`.
    pub fn reselect(&mut self, paths: &[PathBuf], cursor: Option<&Path>) {
        let row_of = |p: &Path| {
            self.visible
                .iter()
                .position(|&i| self.entries.get(i).is_some_and(|e| e.path == p))
        };
        let rows: Vec<usize> = paths.iter().filter_map(|p| row_of(p)).collect();
        let cursor_row = cursor.and_then(row_of);
        self.selection = if rows.is_empty() && cursor_row.is_none() {
            Selection::new()
        } else {
            Selection::from_rows(rows, cursor_row)
        };
    }

    pub fn sort_entries(&mut self) {
        af_fs::sort(&mut self.entries, self.sort, self.ascending);
    }

    /// Row of the entry named `name`.
    pub fn row_named(&self, name: &str) -> Option<usize> {
        self.visible
            .iter()
            .position(|&i| self.entries.get(i).is_some_and(|e| e.name == name))
    }

    /// Total bytes of the selected files (folders count as 0 here).
    pub fn selected_bytes(&self) -> u64 {
        self.selected().iter().map(|e| e.size).sum()
    }
}

/// "Documents" for `/Users/me/Documents`, "Macintosh HD"-style for `/`.
pub fn folder_name(path: &Path) -> String {
    match path.file_name() {
        Some(n) => n.to_string_lossy().into_owned(),
        None => path.display().to_string(),
    }
}

/// macOS bundles (.app, .pages, …) open as documents, like in Finder.
pub fn is_package(path: &Path) -> bool {
    cfg!(target_os = "macos")
        && path
            .extension()
            .map(|e| {
                af_fs::kind::PACKAGE_EXTENSIONS
                    .contains(&e.to_string_lossy().to_lowercase().as_str())
            })
            .unwrap_or(false)
}

/// Whether opening `entry` navigates into it.
pub fn is_folder(entry: &Entry) -> bool {
    entry.is_dir && !is_package(&entry.path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn listing(n: usize) -> Vec<Entry> {
        (0..n)
            .map(|i| Entry {
                name: format!("file{i}"),
                path: PathBuf::from(format!("/d/file{i}")),
                is_dir: false,
                is_symlink: false,
                hidden: false,
                size: i as u64,
                modified: None,
            })
            .collect()
    }

    /// A shorter listing replacing a longer one while row 8 is selected
    /// used to index past the new entries.
    #[test]
    fn shorter_listing_keeps_working() {
        let mut tab = Tab::new(1, PathBuf::from("/d"), ViewMode::List);
        tab.replace_entries(listing(10), false);
        tab.selection.select_only(8);
        assert_eq!(tab.current().unwrap().name, "file8");
        tab.replace_entries(listing(2), false);
        tab.refilter(false);
        assert!(tab.current().is_none());
        assert!(tab.selected().is_empty());
        // Stale indexes are tolerated too, not just after replace_entries.
        tab.selection.select_only(1);
        tab.entries.truncate(1);
        assert!(tab.entry_at(1).is_none());
        tab.refilter(false);
        assert_eq!(tab.visible.len(), 1);
        // The selection survives a reload by path.
        tab.replace_entries(listing(5), false);
        tab.selection.select_only(3);
        tab.replace_entries(listing(5), false);
        assert_eq!(tab.current().unwrap().name, "file3");
    }
}
