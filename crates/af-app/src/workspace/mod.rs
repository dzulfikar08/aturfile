//! The window: tabs, sidebar, toolbar, the file views, preview pane, status
//! bar, command palette and the Clean Up page.

mod columns;
mod dnd;
mod fileops;
mod icons;
mod list;
mod menus;
mod palette;
mod preview;
mod sheet;
mod shell;
mod sidebar;
mod toolbar;
mod watch;

use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

use af_fs::Entry;
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{ActiveTheme as _, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use crate::actions::*;
use crate::appearance;
use crate::cleanup::Cleanup;
use crate::settings::Settings;
use crate::tab::{Page, Tab, ViewMode, is_folder};

pub use fileops::FileOps;

/// How the window starts (command-line flags).
#[derive(Debug, Clone, Default)]
pub struct StartOptions {
    pub dir: PathBuf,
    pub cleanup: bool,
    pub view: Option<ViewMode>,
    pub preview: bool,
    /// Select this child of `dir` once it is listed.
    pub select: Option<String>,
    pub palette: bool,
    /// Open the search dialog with this query.
    pub search: Option<String>,
    /// Open a sheet: `about` or `pro`.
    pub sheet: Option<String>,
}

/// A panel edge or list column being dragged.
#[derive(Debug, Clone, Copy)]
pub(crate) enum Drag {
    Sidebar,
    Preview,
    /// A list column's left edge: `ix` into Modified, Size, Kind.
    Column {
        ix: usize,
        start_x: Pixels,
        start_w: f32,
    },
}

/// What to select once a folder is listed.
pub(crate) enum Select {
    Nothing,
    Name(String),
    /// Keep these paths selected (a reload).
    Keep(Vec<PathBuf>, Option<PathBuf>),
    /// The folder changed on disk: keep the selection and don't scroll.
    Refresh(Vec<PathBuf>, Option<PathBuf>),
}

pub struct Workspace {
    pub(crate) focus: FocusHandle,
    pub(crate) tabs: Vec<Tab>,
    pub(crate) active: usize,
    next_tab_id: u64,
    pub(crate) show_hidden: bool,
    pub(crate) settings: Settings,
    pub(crate) volumes: Vec<af_mac::Volume>,
    pub(crate) filter: Entity<InputState>,
    pub(crate) location: Entity<InputState>,
    pub(crate) editing_location: bool,
    pub(crate) palette: Option<palette::PaletteState>,
    /// The file-name index behind Cmd-K, built on first use.
    pub(crate) search_index: Option<palette::SearchIndex>,
    pub(crate) sheet: Option<sheet::Sheet>,
    pub(crate) preview: preview::Preview,
    pub(crate) drag: Option<Drag>,
    pub cleanup: Cleanup,
    pub(crate) ops: FileOps,
    pub(crate) watch: watch::Watch,
    /// Folders the App Sandbox lets us read (unused outside the sandbox).
    pub(crate) grants: af_mac::Grants,
    /// A message for the status bar ("Moved 3 items to Trash").
    pub(crate) notice: Option<SharedString>,
    /// A right-click landed on a row (the background menu stays shut).
    pub(crate) right_click_on_row: bool,
    /// A plain mouse-down on a row already in a multi-selection: narrow the
    /// selection to it on release unless the press became a drag.
    pending_select: Option<usize>,
    /// List view: which of Modified, Size, Kind fit beside the names.
    pub(crate) list_cols: [bool; 3],
    /// AF_DEBUG: last logged window metrics.
    debug_last: Option<String>,
    _subscriptions: Vec<Subscription>,
    _poll: Task<()>,
}

impl Workspace {
    pub fn new(opts: StartOptions, window: &mut Window, cx: &mut Context<Self>) -> Self {
        let settings = Settings::load();
        let focus = cx.focus_handle();
        window.focus(&focus, cx);
        appearance::set_system_dark(appearance::is_dark(window.appearance()), cx);

        let filter = cx.new(|cx| InputState::new(window, cx).placeholder("Filter"));
        let location = cx.new(|cx| {
            InputState::new(window, cx).placeholder("Go to folder: a path, ~ or ~/Downloads")
        });
        let subscriptions = vec![
            cx.subscribe_in(&filter, window, Self::on_filter_input),
            cx.subscribe_in(&location, window, Self::on_location_input),
            cx.observe_window_appearance(window, |_, window, cx| {
                appearance::set_system_dark(appearance::is_dark(window.appearance()), cx);
            }),
        ];

        // Disks come and go; their free space changes. Cheap to re-read.
        let poll = cx.spawn(async move |this, cx| {
            loop {
                let vols = cx
                    .background_executor()
                    .spawn(async { af_mac::volumes() })
                    .await;
                if this
                    .update(cx, |ws, cx| {
                        if ws.volumes != vols {
                            ws.volumes = vols;
                            cx.notify();
                        }
                    })
                    .is_err()
                {
                    break;
                }
                // Mounts arrive through the /Volumes watch; this keeps the
                // free space current.
                cx.background_executor()
                    .timer(Duration::from_secs(20))
                    .await;
            }
        });

        let ops = FileOps::new(window, cx);
        let view = opts.view.unwrap_or_else(|| ViewMode::parse(&settings.view));
        let mut this = Self {
            focus,
            tabs: vec![Tab::new(1, opts.dir.clone(), view)],
            active: 0,
            next_tab_id: 2,
            show_hidden: settings.show_hidden,
            volumes: Vec::new(),
            filter,
            location,
            editing_location: false,
            palette: None,
            search_index: None,
            sheet: None,
            preview: preview::Preview::default(),
            drag: None,
            cleanup: Cleanup::default(),
            ops,
            watch: watch::Watch::new(cx),
            grants: af_mac::Grants::restore(),
            notice: None,
            right_click_on_row: false,
            pending_select: None,
            list_cols: [true; 3],
            debug_last: None,
            _subscriptions: subscriptions,
            _poll: poll,
            settings,
        };
        if opts.preview {
            this.settings.show_preview = true;
        }
        let select = opts.select.map_or(Select::Nothing, Select::Name);
        this.load(0, opts.dir, select, cx);
        if opts.cleanup {
            this.show_cleanup(cx);
        }
        if opts.palette {
            this.open_palette(palette::Mode::Commands, window, cx);
        }
        match opts.sheet.as_deref() {
            Some("about") => this.open_sheet(sheet::Sheet::About, cx),
            Some("pro") => this.open_sheet(sheet::Sheet::Pro, cx),
            _ => {}
        }
        if let Some(query) = opts.search {
            this.open_palette(palette::Mode::Search, window, cx);
            this.set_palette_text(query, window, cx);
        }
        this
    }

    // ---- state helpers ----------------------------------------------------

    pub(crate) fn tab(&self) -> &Tab {
        &self.tabs[self.active]
    }

    pub(crate) fn tab_mut(&mut self) -> &mut Tab {
        &mut self.tabs[self.active]
    }

    fn tab_index(&self, id: u64) -> Option<usize> {
        self.tabs.iter().position(|t| t.id == id)
    }

    pub(crate) fn save_settings(&mut self) {
        self.settings.show_hidden = self.show_hidden;
        if let Err(e) = self.settings.save() {
            self.notice = Some(format!("Couldn't save settings: {e}").into());
        }
    }

    pub(crate) fn set_notice(&mut self, text: impl Into<SharedString>, cx: &mut Context<Self>) {
        self.notice = Some(text.into());
        cx.notify();
    }

    /// The file list (not a text field) has the keyboard.
    pub(crate) fn list_focused(&self, window: &Window) -> bool {
        self.focus.is_focused(window)
    }

    /// The filter field has the keyboard.
    pub(crate) fn filter_focused(&self, window: &Window, cx: &App) -> bool {
        self.filter.focus_handle(cx).is_focused(window)
    }

    /// Selection keys apply: the list or the filter field has the keyboard.
    fn selection_keys(&self, window: &Window, cx: &App) -> bool {
        self.list_focused(window) || self.filter_focused(window, cx)
    }

    pub(crate) fn focus_list(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        window.focus(&self.focus, cx);
    }

    // ---- loading and navigation --------------------------------------------

    /// List `dir` off the UI thread into tab `ix`, then select `select`.
    pub(crate) fn load(&mut self, ix: usize, dir: PathBuf, select: Select, cx: &mut Context<Self>) {
        let Some(tab) = self.tabs.get_mut(ix) else {
            return;
        };
        let id = tab.id;
        let read = dir.clone();
        let task = cx.background_executor().spawn(async move {
            let started = Instant::now();
            let listed = af_fs::list_dir(&read);
            (listed, started.elapsed().as_millis())
        });
        tab.load_task = Some(cx.spawn(async move |this, cx| {
            let (listed, ms) = task.await;
            let _ = this.update(cx, |ws, cx| ws.finish_load(id, dir, listed, ms, select, cx));
        }));
    }

    fn finish_load(
        &mut self,
        id: u64,
        dir: PathBuf,
        listed: std::io::Result<Vec<Entry>>,
        ms: u128,
        select: Select,
        cx: &mut Context<Self>,
    ) {
        let Some(ix) = self.tab_index(id) else {
            return;
        };
        let show_hidden = self.show_hidden;
        let tab = &mut self.tabs[ix];
        tab.cwd = dir;
        tab.loaded = true;
        tab.load_ms = ms;
        match listed {
            Ok(entries) => {
                tab.replace_entries(entries, show_hidden);
                tab.error = None;
            }
            Err(e) => {
                tab.replace_entries(Vec::new(), show_hidden);
                tab.error = Some(format!("Can't open {}: {e}", tab.cwd.display()).into());
            }
        }
        tab.refilter(show_hidden);
        let mut quiet = false;
        match select {
            Select::Nothing => tab.selection.clear(),
            Select::Name(name) => {
                tab.selection = tab
                    .row_named(&name)
                    .map(af_fs::Selection::single)
                    .unwrap_or_default();
            }
            Select::Keep(paths, cursor) => tab.reselect(&paths, cursor.as_deref()),
            Select::Refresh(paths, cursor) => {
                tab.reselect(&paths, cursor.as_deref());
                quiet = true;
            }
        }
        if !quiet {
            let row = tab.selection.cursor().unwrap_or(0);
            tab.scroll.scroll_to_item(row, ScrollStrategy::Center);
        }
        // Miller columns re-read what they show.
        tab.listings.clear();
        tab.listing_tasks.clear();
        if ix == self.active {
            self.editing_location = false;
        }
        cx.notify();
    }

    /// Go to `dir` in the active tab, remembering where we were.
    pub(crate) fn go(&mut self, dir: PathBuf, select: Option<String>, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        tab.page = Page::Files;
        if dir == tab.cwd {
            if let Some(name) = select
                && let Some(row) = tab.row_named(&name)
            {
                tab.selection.select_only(row);
                tab.scroll.scroll_to_item(row, ScrollStrategy::Nearest);
            }
            cx.notify();
            return;
        }
        let from = std::mem::replace(&mut tab.cwd, dir.clone());
        tab.back.push(from);
        tab.forward.clear();
        tab.query.clear();
        let select = select.map_or(Select::Nothing, Select::Name);
        self.load(self.active, dir, select, cx);
        cx.notify();
    }

    /// Re-read the active folder, keeping the selection.
    pub(crate) fn reload(&mut self, cx: &mut Context<Self>) {
        let tab = self.tab();
        let keep = Select::Keep(tab.selected_paths(), tab.current().map(|e| e.path.clone()));
        let dir = tab.cwd.clone();
        self.load(self.active, dir, keep, cx);
    }

    /// Re-read every tab showing one of `dirs`.
    pub(crate) fn reload_dirs(&mut self, dirs: &[PathBuf], cx: &mut Context<Self>) {
        for ix in 0..self.tabs.len() {
            let tab = &self.tabs[ix];
            if dirs.iter().any(|d| d == &tab.cwd) {
                let keep =
                    Select::Keep(tab.selected_paths(), tab.current().map(|e| e.path.clone()));
                let dir = tab.cwd.clone();
                self.load(ix, dir, keep, cx);
            }
        }
    }

    pub(crate) fn up(&mut self, cx: &mut Context<Self>) {
        let cwd = self.tab().cwd.clone();
        let Some(parent) = cwd.parent().map(Path::to_path_buf) else {
            return;
        };
        let child = cwd.file_name().map(|n| n.to_string_lossy().into_owned());
        self.go(parent, child, cx);
    }

    pub(crate) fn back(&mut self, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        if let Some(dir) = tab.back.pop() {
            let from = std::mem::replace(&mut tab.cwd, dir.clone());
            let child = from
                .strip_prefix(&dir)
                .ok()
                .and_then(|r| r.components().next())
                .map(|c| c.as_os_str().to_string_lossy().into_owned());
            tab.forward.push(from);
            tab.page = Page::Files;
            tab.query.clear();
            let select = child.map_or(Select::Nothing, Select::Name);
            self.load(self.active, dir, select, cx);
        }
    }

    pub(crate) fn forward(&mut self, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        if let Some(dir) = tab.forward.pop() {
            let from = std::mem::replace(&mut tab.cwd, dir.clone());
            tab.back.push(from);
            tab.page = Page::Files;
            tab.query.clear();
            self.load(self.active, dir, Select::Nothing, cx);
        }
    }

    /// Open the entry at `row`: folders navigate, everything else opens in
    /// its default app.
    pub(crate) fn open_row(&mut self, row: usize, cx: &mut Context<Self>) {
        let Some(entry) = self.tab().entry_at(row).cloned() else {
            return;
        };
        if is_folder(&entry) {
            self.go(entry.path, None, cx);
        } else {
            cx.open_with_system(&entry.path);
        }
    }

    /// Open the selection: one folder navigates; files open in their apps.
    pub(crate) fn open_selected(&mut self, cx: &mut Context<Self>) {
        let selected: Vec<Entry> = self.tab().selected().into_iter().cloned().collect();
        if let [only] = selected.as_slice()
            && is_folder(only)
        {
            self.go(only.path.clone(), None, cx);
            return;
        }
        for entry in selected {
            if is_folder(&entry) {
                self.new_tab_at(entry.path.clone(), cx);
            } else {
                cx.open_with_system(&entry.path);
            }
        }
    }

    // ---- selection --------------------------------------------------------

    pub(crate) fn scroll_to_cursor(&mut self) {
        let tab = self.tab_mut();
        if let Some(row) = tab.selection.cursor() {
            let target = if tab.view == ViewMode::Icons {
                row / tab.icon_cols.max(1)
            } else {
                row
            };
            tab.scroll.scroll_to_item(target, ScrollStrategy::Nearest);
        }
    }

    fn move_selection(&mut self, delta: isize, extend: bool, cx: &mut Context<Self>) {
        if self.tab().page == Page::Cleanup {
            self.cleanup.move_by(delta.signum(), cx);
            return;
        }
        let tab = self.tab_mut();
        let len = tab.visible.len();
        tab.selection.move_by(delta, len, extend);
        self.scroll_to_cursor();
        cx.notify();
    }

    fn jump_selection(&mut self, row: usize, extend: bool, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        let len = tab.visible.len();
        tab.selection.move_to(row, len, extend);
        self.scroll_to_cursor();
        cx.notify();
    }

    /// A click on `row` with `modifiers`: Cmd toggles, Shift extends.
    pub(crate) fn click_row(
        &mut self,
        row: usize,
        modifiers: Modifiers,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        self.focus_list(window, cx);
        self.editing_location = false;
        self.pending_select = None;
        let tab = self.tab_mut();
        if modifiers.platform || modifiers.control {
            tab.selection.toggle(row);
        } else if modifiers.shift {
            tab.selection.extend_to(row);
        } else if tab.selection.contains(row) && tab.selection.count() > 1 {
            // Keep the selection so it can be dragged; see `release_row`.
            self.pending_select = Some(row);
        } else {
            tab.selection.select_only(row);
        }
        cx.notify();
    }

    /// A click (press and release, no drag) on `row`.
    pub(crate) fn release_row(&mut self, row: usize, cx: &mut Context<Self>) {
        if self.pending_select.take() == Some(row) {
            self.tab_mut().selection.select_only(row);
            cx.notify();
        }
    }

    /// A right-click on `row`: select it unless it already is.
    pub(crate) fn right_click_row(&mut self, row: usize, cx: &mut Context<Self>) {
        self.right_click_on_row = true;
        let tab = self.tab_mut();
        if !tab.selection.contains(row) {
            tab.selection.select_only(row);
        }
        cx.notify();
    }

    /// A right-click on empty space in a view: nothing selected.
    pub(crate) fn right_click_background(&mut self, cx: &mut Context<Self>) {
        if std::mem::take(&mut self.right_click_on_row) {
            return;
        }
        self.tab_mut().selection.clear();
        cx.notify();
    }

    fn set_query(&mut self, query: String, cx: &mut Context<Self>) {
        let show_hidden = self.show_hidden;
        let tab = self.tab_mut();
        tab.query = query;
        tab.refilter(show_hidden);
        tab.selection = if tab.visible.is_empty() || tab.query.is_empty() {
            tab.selection.clone()
        } else {
            af_fs::Selection::single(0)
        };
        tab.scroll.scroll_to_item(0, ScrollStrategy::Top);
        cx.notify();
    }

    // ---- tabs -------------------------------------------------------------

    pub(crate) fn new_tab_at(&mut self, dir: PathBuf, cx: &mut Context<Self>) {
        // More than one tab is Pro in App Store builds.
        if !self.require_pro(cx) {
            return;
        }
        let id = self.next_tab_id;
        self.next_tab_id += 1;
        let view = self.tab().view;
        self.tabs
            .insert(self.active + 1, Tab::new(id, dir.clone(), view));
        self.active += 1;
        self.editing_location = false;
        self.load(self.active, dir, Select::Nothing, cx);
        cx.notify();
    }

    pub(crate) fn activate_tab(&mut self, ix: usize, cx: &mut Context<Self>) {
        if ix < self.tabs.len() {
            self.active = ix;
            self.editing_location = false;
            cx.notify();
        }
    }

    pub(crate) fn close_tab(&mut self, ix: usize, window: &mut Window, cx: &mut Context<Self>) {
        if self.tabs.len() <= 1 {
            window.remove_window();
            return;
        }
        self.tabs.remove(ix);
        if self.active >= self.tabs.len() || self.active > ix {
            self.active = self.active.saturating_sub(1);
        }
        self.editing_location = false;
        cx.notify();
    }

    // ---- views and panels -------------------------------------------------

    pub(crate) fn set_view(&mut self, view: ViewMode, cx: &mut Context<Self>) {
        let tab = self.tab_mut();
        tab.page = Page::Files;
        tab.view = view;
        self.settings.view = view.as_str().into();
        self.scroll_to_cursor();
        self.save_settings();
        cx.notify();
    }

    pub(crate) fn show_cleanup(&mut self, cx: &mut Context<Self>) {
        self.tab_mut().page = Page::Cleanup;
        if !self.cleanup.started() && self.grants.covers(&af_clean::paths::home()) {
            Cleanup::start_scan(self, cx);
        }
        cx.notify();
    }

    pub(crate) fn toggle_sidebar_panel(&mut self, cx: &mut Context<Self>) {
        self.settings.show_sidebar = !self.settings.show_sidebar;
        self.save_settings();
        cx.notify();
    }

    pub(crate) fn toggle_preview_panel(&mut self, cx: &mut Context<Self>) {
        self.settings.show_preview = !self.settings.show_preview;
        self.save_settings();
        cx.notify();
    }

    pub(crate) fn toggle_hidden_files(&mut self, cx: &mut Context<Self>) {
        self.show_hidden = !self.show_hidden;
        let show = self.show_hidden;
        for tab in &mut self.tabs {
            tab.refilter(show);
        }
        self.save_settings();
        cx.notify();
    }

    pub(crate) fn toggle_pin(&mut self, path: &Path, cx: &mut Context<Self>) {
        let pinned = self.settings.toggle_pin(path);
        self.save_settings();
        let name = crate::tab::folder_name(path);
        self.set_notice(
            if pinned {
                format!("Pinned \u{201c}{name}\u{201d} to the sidebar")
            } else {
                format!("Unpinned \u{201c}{name}\u{201d}")
            },
            cx,
        );
    }

    pub(crate) fn copy_paths(&mut self, paths: &[PathBuf], cx: &mut Context<Self>) {
        if paths.is_empty() {
            return;
        }
        let text = paths
            .iter()
            .map(|p| p.display().to_string())
            .collect::<Vec<_>>()
            .join("\n");
        cx.write_to_clipboard(ClipboardItem::new_string(text));
        self.set_notice(
            if paths.len() == 1 {
                "Copied the path".to_string()
            } else {
                format!("Copied {} paths", paths.len())
            },
            cx,
        );
    }

    /// The paths an item command acts on: the selection, or the folder
    /// itself when nothing is selected.
    pub(crate) fn targets(&self) -> Vec<PathBuf> {
        let sel = self.tab().selected_paths();
        if sel.is_empty() {
            vec![self.tab().cwd.clone()]
        } else {
            sel
        }
    }

    pub(crate) fn quick_look_selected(&mut self, cx: &mut Context<Self>) {
        if let Some(entry) = self.tab().current() {
            quick_look(&entry.path.clone(), cx);
        }
    }

    // ---- location field -----------------------------------------------------

    pub(crate) fn start_editing_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().page != Page::Files {
            return;
        }
        let shown = af_fs::location::display(&self.tab().cwd, &af_clean::paths::home());
        self.editing_location = true;
        self.location.update(cx, |input, cx| {
            input.set_value(shown, window, cx);
            input.focus(window, cx);
            input.select_all(window, cx);
        });
        cx.notify();
    }

    pub(crate) fn stop_editing_location(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.editing_location {
            self.editing_location = false;
            self.focus_list(window, cx);
            cx.notify();
        }
    }

    fn on_location_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::PressEnter { .. } => {
                let typed = input.read(cx).value().to_string();
                let home = af_clean::paths::home();
                let target = af_fs::location::expand(&typed, &self.tab().cwd, &home);
                self.editing_location = false;
                self.focus_list(window, cx);
                if target.is_dir() {
                    self.go(target, None, cx);
                } else if let Some(parent) = target.parent().filter(|p| p.is_dir()) {
                    // A file: show it in its folder.
                    let name = target.file_name().map(|n| n.to_string_lossy().into_owned());
                    self.go(parent.to_path_buf(), name, cx);
                } else {
                    self.set_notice(format!("No folder at {}", target.display()), cx);
                }
            }
            InputEvent::Blur if self.editing_location => {
                self.editing_location = false;
                cx.notify();
            }
            _ => {}
        }
    }

    fn on_filter_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let value = input.read(cx).value().to_string();
                if value != self.tab().query {
                    self.set_query(value, cx);
                }
            }
            InputEvent::PressEnter { .. } => {
                self.focus_list(window, cx);
                if self.tab().current().is_some() {
                    self.open_selected(cx);
                }
            }
            _ => {}
        }
    }

    /// Keep the filter field showing the active tab's query.
    fn sync_filter(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let query = self.tab().query.clone();
        if self.filter.read(cx).value().as_ref() != query.as_str() {
            self.filter
                .update(cx, |input, cx| input.set_value(query, window, cx));
        }
    }

    // ---- actions ----------------------------------------------------------

    fn on_new_tab(&mut self, _: &NewTab, _: &mut Window, cx: &mut Context<Self>) {
        let dir = self.tab().cwd.clone();
        self.new_tab_at(dir, cx);
    }
    fn on_close_tab(&mut self, _: &CloseTab, window: &mut Window, cx: &mut Context<Self>) {
        self.close_tab(self.active, window, cx);
    }
    fn on_next_tab(&mut self, _: &NextTab, _: &mut Window, cx: &mut Context<Self>) {
        let ix = (self.active + 1) % self.tabs.len();
        self.activate_tab(ix, cx);
    }
    fn on_prev_tab(&mut self, _: &PrevTab, _: &mut Window, cx: &mut Context<Self>) {
        let ix = (self.active + self.tabs.len() - 1) % self.tabs.len();
        self.activate_tab(ix, cx);
    }
    fn on_toggle_palette(
        &mut self,
        _: &TogglePalette,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.palette.is_some() {
            self.close_palette(window, cx);
        } else {
            self.open_palette(palette::Mode::Search, window, cx);
        }
    }
    fn on_show_about(&mut self, _: &ShowAbout, _: &mut Window, cx: &mut Context<Self>) {
        self.open_sheet(sheet::Sheet::About, cx);
    }
    fn on_show_commands(&mut self, _: &ShowCommands, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette(palette::Mode::Commands, window, cx);
    }
    fn on_open_settings(&mut self, _: &OpenSettings, window: &mut Window, cx: &mut Context<Self>) {
        self.open_palette(palette::Mode::Themes, window, cx);
    }
    fn on_cancel(&mut self, _: &Cancel, window: &mut Window, cx: &mut Context<Self>) {
        if self.sheet.is_some() {
            self.close_sheet(cx);
        } else if self.palette.is_some() {
            self.close_palette(window, cx);
        } else if self.ops.renaming.is_some() {
            self.cancel_rename(window, cx);
        } else if self.editing_location {
            self.stop_editing_location(window, cx);
        } else if self.filter_focused(window, cx) {
            self.set_query(String::new(), cx);
            self.focus_list(window, cx);
        } else if !self.tab().query.is_empty() {
            self.set_query(String::new(), cx);
        } else if !self.tab().selection.is_empty() && self.list_focused(window) {
            self.tab_mut().selection.clear();
            cx.notify();
        } else {
            cx.propagate();
        }
    }
    fn on_toggle_sidebar(&mut self, _: &ToggleSidebar, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_sidebar_panel(cx);
    }
    fn on_toggle_preview(&mut self, _: &TogglePreview, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_preview_panel(cx);
    }
    fn on_get_info(&mut self, _: &GetInfo, _: &mut Window, cx: &mut Context<Self>) {
        if !self.settings.show_preview {
            self.toggle_preview_panel(cx);
        }
    }
    fn on_show_list(&mut self, _: &ShowList, _: &mut Window, cx: &mut Context<Self>) {
        self.set_view(ViewMode::List, cx);
    }
    fn on_show_columns(&mut self, _: &ShowColumns, _: &mut Window, cx: &mut Context<Self>) {
        self.set_view(ViewMode::Columns, cx);
    }
    fn on_show_icons(&mut self, _: &ShowIcons, _: &mut Window, cx: &mut Context<Self>) {
        self.set_view(ViewMode::Icons, cx);
    }
    fn on_show_cleanup(&mut self, _: &ShowCleanup, _: &mut Window, cx: &mut Context<Self>) {
        self.show_cleanup(cx);
    }
    fn on_focus_filter(&mut self, _: &FocusFilter, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().page == Page::Files {
            self.editing_location = false;
            self.filter.update(cx, |input, cx| {
                input.focus(window, cx);
                input.select_all(window, cx);
            });
            cx.notify();
        }
    }
    fn on_edit_location(&mut self, _: &EditLocation, window: &mut Window, cx: &mut Context<Self>) {
        self.start_editing_location(window, cx);
    }

    fn on_select_next(&mut self, _: &SelectNext, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selection_keys(window, cx) {
            return cx.propagate();
        }
        let step = self.vertical_step();
        self.move_selection(step, false, cx);
    }
    fn on_select_prev(&mut self, _: &SelectPrev, window: &mut Window, cx: &mut Context<Self>) {
        if !self.selection_keys(window, cx) {
            return cx.propagate();
        }
        let step = self.vertical_step();
        self.move_selection(-step, false, cx);
    }
    fn on_extend_next(&mut self, _: &ExtendNext, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        let step = self.vertical_step();
        self.move_selection(step, true, cx);
    }
    fn on_extend_prev(&mut self, _: &ExtendPrev, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        let step = self.vertical_step();
        self.move_selection(-step, true, cx);
    }
    fn on_select_left(&mut self, _: &SelectLeft, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        match self.tab().view {
            ViewMode::Icons => self.move_selection(-1, false, cx),
            ViewMode::Columns => self.up(cx),
            ViewMode::List => cx.propagate(),
        }
    }
    fn on_select_right(&mut self, _: &SelectRight, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        match self.tab().view {
            ViewMode::Icons => self.move_selection(1, false, cx),
            ViewMode::Columns => self.columns_enter(cx),
            ViewMode::List => cx.propagate(),
        }
    }
    fn on_extend_left(&mut self, _: &ExtendLeft, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_focused(window) && self.tab().view == ViewMode::Icons {
            self.move_selection(-1, true, cx);
        } else {
            cx.propagate();
        }
    }
    fn on_extend_right(&mut self, _: &ExtendRight, window: &mut Window, cx: &mut Context<Self>) {
        if self.list_focused(window) && self.tab().view == ViewMode::Icons {
            self.move_selection(1, true, cx);
        } else {
            cx.propagate();
        }
    }
    fn on_select_first(&mut self, _: &SelectFirst, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        self.jump_selection(0, false, cx);
    }
    fn on_select_last(&mut self, _: &SelectLast, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        self.jump_selection(usize::MAX, false, cx);
    }
    fn on_page_down(&mut self, _: &PageDown, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        let step = self.page_step(window, cx);
        self.move_selection(step, false, cx);
    }
    fn on_page_up(&mut self, _: &PageUp, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        let step = self.page_step(window, cx);
        self.move_selection(-step, false, cx);
    }
    fn on_select_all(&mut self, _: &SelectAll, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        let tab = self.tab_mut();
        let len = tab.visible.len();
        tab.selection.select_all(len);
        cx.notify();
    }

    /// Rows per Up/Down: one, or a row of icons.
    fn vertical_step(&self) -> isize {
        let tab = self.tab();
        if tab.view == ViewMode::Icons {
            tab.icon_cols.max(1) as isize
        } else {
            1
        }
    }

    fn page_step(&self, window: &Window, cx: &App) -> isize {
        let row = f32::from(appearance::density(cx).row_height());
        let rows = (f32::from(window.viewport_size().height) / row) as isize - 6;
        rows.max(1) * self.vertical_step()
    }

    fn on_open(&mut self, _: &Open, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) {
            return cx.propagate();
        }
        match self.tab().page {
            Page::Files => self.open_selected(cx),
            Page::Cleanup => self.cleanup.toggle_selected(cx),
        }
    }
    fn on_parent(&mut self, _: &Parent, _: &mut Window, cx: &mut Context<Self>) {
        self.up(cx);
    }
    fn on_back(&mut self, _: &Back, _: &mut Window, cx: &mut Context<Self>) {
        self.back(cx);
    }
    fn on_forward(&mut self, _: &Forward, _: &mut Window, cx: &mut Context<Self>) {
        self.forward(cx);
    }
    fn on_go_home(&mut self, _: &GoHome, _: &mut Window, cx: &mut Context<Self>) {
        self.go(af_clean::paths::home(), None, cx);
    }
    fn on_reload(&mut self, _: &Reload, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab().page == Page::Cleanup {
            Cleanup::start_scan(self, cx);
        } else {
            self.reload(cx);
        }
    }
    fn on_toggle_hidden(&mut self, _: &ToggleHidden, _: &mut Window, cx: &mut Context<Self>) {
        self.toggle_hidden_files(cx);
    }
    fn on_reveal(&mut self, _: &Reveal, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(path) = self.targets().first() {
            cx.reveal_path(path);
        }
    }
    fn on_copy_path(&mut self, _: &CopyPath, _: &mut Window, cx: &mut Context<Self>) {
        let paths = self.targets();
        self.copy_paths(&paths, cx);
    }
    fn on_quick_look(&mut self, _: &QuickLook, window: &mut Window, cx: &mut Context<Self>) {
        if !self.list_focused(window) || self.tab().page != Page::Files {
            return cx.propagate();
        }
        if self.tab().query.is_empty() {
            self.quick_look_selected(cx);
        } else {
            // Type-to-filter: spaces are part of names.
            let mut q = self.tab().query.clone();
            q.push(' ');
            self.set_query(q, cx);
        }
    }
    fn on_stage(&mut self, _: &StageSelected, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab().page == Page::Cleanup {
            self.cleanup.toggle_selected(cx);
        }
    }
    fn on_empty_queue(&mut self, _: &EmptyQueue, _: &mut Window, cx: &mut Context<Self>) {
        if self.tab().page == Page::Cleanup {
            Cleanup::trash_queue(self, cx);
        }
    }
    fn on_palette_up(&mut self, _: &PaletteUp, _: &mut Window, cx: &mut Context<Self>) {
        self.palette_move(-1, cx);
    }
    fn on_palette_down(&mut self, _: &PaletteDown, _: &mut Window, cx: &mut Context<Self>) {
        self.palette_move(1, cx);
    }

    /// Type-to-filter: printable keys build the query, Backspace edits it (or
    /// goes up a folder when it's empty). Only while the list has focus.
    fn on_key(&mut self, event: &KeyDownEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.tab().page != Page::Files || !self.list_focused(window) {
            return;
        }
        let ks = &event.keystroke;
        if ks.modifiers.platform || ks.modifiers.control || ks.modifiers.alt {
            return;
        }
        let mut query = self.tab().query.clone();
        match ks.key.as_str() {
            "backspace" => {
                if query.pop().is_none() {
                    self.up(cx);
                    cx.stop_propagation();
                    return;
                }
            }
            _ => match ks.key_char.as_deref() {
                Some(c) if !c.is_empty() && !c.chars().any(char::is_control) => query.push_str(c),
                _ => return,
            },
        }
        self.set_query(query, cx);
        cx.stop_propagation();
    }

    // ---- panel resizing -----------------------------------------------------

    fn on_drag_move(
        &mut self,
        event: &MouseMoveEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        let Some(drag) = self.drag else {
            return;
        };
        if event.pressed_button != Some(MouseButton::Left) {
            self.end_drag(cx);
            return;
        }
        let x = f32::from(event.position.x);
        match drag {
            Drag::Sidebar => {
                self.settings.sidebar_width = x.clamp(SIDEBAR_MIN, SIDEBAR_MAX);
            }
            Drag::Preview => {
                let w = f32::from(window.viewport_size().width) - x;
                self.settings.preview_width = w.clamp(PREVIEW_MIN, PREVIEW_MAX);
            }
            Drag::Column {
                ix,
                start_x,
                start_w,
            } => {
                let w = start_w + f32::from(start_x) - x;
                self.settings.columns[ix] = w.clamp(COL_MIN, COL_MAX);
            }
        }
        cx.notify();
    }

    fn end_drag(&mut self, cx: &mut Context<Self>) {
        if self.drag.take().is_some() {
            self.save_settings();
            cx.notify();
        }
    }

    /// An invisible strip on a panel edge that starts a resize.
    pub(crate) fn resize_handle(
        &self,
        id: &'static str,
        drag: Drag,
        cx: &mut Context<Self>,
    ) -> Stateful<Div> {
        let t = tokens(cx);
        div()
            .id(id)
            .absolute()
            .top_0()
            .bottom_0()
            .w(RESIZE_HANDLE)
            .cursor(CursorStyle::ResizeLeftRight)
            .hover(|s| s.bg(t.accent_soft))
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |ws, _: &MouseDownEvent, _, cx| {
                    ws.drag = Some(drag);
                    cx.stop_propagation();
                    cx.notify();
                }),
            )
    }

    fn debug_metrics(&mut self, window: &Window) {
        if std::env::var_os("AF_DEBUG").is_none() {
            return;
        }
        let now = format!(
            "scale {} viewport {:?}",
            window.scale_factor(),
            window.viewport_size()
        );
        if self.debug_last.as_deref() != Some(now.as_str()) {
            eprintln!("aturfile: {now}");
            self.debug_last = Some(now);
        }
    }

    // ---- sandbox grants ---------------------------------------------------

    /// App Store builds: ask for a folder in the open panel, then keep it.
    pub(crate) fn request_access(&mut self, cx: &mut Context<Self>) {
        let answer = cx.prompt_for_paths(PathPromptOptions {
            files: false,
            directories: true,
            multiple: true,
            prompt: Some("Allow Access".into()),
        });
        cx.spawn(async move |this, cx| {
            let Ok(Ok(Some(paths))) = answer.await else {
                return;
            };
            let _ = this.update(cx, |ws, cx| {
                for path in &paths {
                    if let Err(e) = ws.grants.add(path) {
                        ws.notice = Some(format!("Couldn't keep access: {e}").into());
                    }
                }
                ws.reload(cx);
                if ws.tab().page == Page::Cleanup {
                    ws.show_cleanup(cx);
                }
                cx.notify();
            });
        })
        .detach();
    }
}

fn quick_look(path: &Path, cx: &mut App) {
    if cfg!(target_os = "macos") {
        // TODO(pro): an in-window QLPreviewPanel instead of qlmanage.
        let _ = std::process::Command::new("qlmanage")
            .arg("-p")
            .arg(path)
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .spawn();
    } else {
        cx.open_with_system(path);
    }
}

impl Focusable for Workspace {
    fn focus_handle(&self, _: &App) -> FocusHandle {
        self.focus.clone()
    }
}

impl Render for Workspace {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        window.set_rem_size(REM);
        self.debug_metrics(window);
        self.sync_filter(window, cx);
        self.sync_watches();
        let title = self.tab().title();
        window.set_window_title(&title);
        let t = tokens(cx);

        let main = match self.tab().page {
            Page::Files => self.render_files(window, cx).into_any_element(),
            Page::Cleanup if !self.grants.covers(&af_clean::paths::home()) => {
                self.render_grant(&af_clean::paths::home(), cx)
            }
            Page::Cleanup => self.render_cleanup(cx).into_any_element(),
        };
        let sidebar_open = self.settings.show_sidebar;
        let sidebar_w = px(self.settings.sidebar_width);
        let dragging = self.drag.is_some();

        v_flex()
            .id("workspace")
            .key_context(WORKSPACE)
            .track_focus(&self.focus)
            .on_key_down(cx.listener(Self::on_key))
            .on_action(cx.listener(Self::on_new_tab))
            .on_action(cx.listener(Self::on_close_tab))
            .on_action(cx.listener(Self::on_next_tab))
            .on_action(cx.listener(Self::on_prev_tab))
            .on_action(cx.listener(Self::on_toggle_palette))
            .on_action(cx.listener(Self::on_show_commands))
            .on_action(cx.listener(Self::on_show_about))
            .on_action(cx.listener(Self::on_open_settings))
            .on_action(cx.listener(Self::on_cancel))
            .on_action(cx.listener(Self::on_toggle_sidebar))
            .on_action(cx.listener(Self::on_toggle_preview))
            .on_action(cx.listener(Self::on_get_info))
            .on_action(cx.listener(Self::on_show_list))
            .on_action(cx.listener(Self::on_show_columns))
            .on_action(cx.listener(Self::on_show_icons))
            .on_action(cx.listener(Self::on_show_cleanup))
            .on_action(cx.listener(Self::on_focus_filter))
            .on_action(cx.listener(Self::on_edit_location))
            .on_action(cx.listener(Self::on_select_next))
            .on_action(cx.listener(Self::on_select_prev))
            .on_action(cx.listener(Self::on_extend_next))
            .on_action(cx.listener(Self::on_extend_prev))
            .on_action(cx.listener(Self::on_select_left))
            .on_action(cx.listener(Self::on_select_right))
            .on_action(cx.listener(Self::on_extend_left))
            .on_action(cx.listener(Self::on_extend_right))
            .on_action(cx.listener(Self::on_select_first))
            .on_action(cx.listener(Self::on_select_last))
            .on_action(cx.listener(Self::on_page_down))
            .on_action(cx.listener(Self::on_page_up))
            .on_action(cx.listener(Self::on_select_all))
            .on_action(cx.listener(Self::on_open))
            .on_action(cx.listener(Self::on_parent))
            .on_action(cx.listener(Self::on_back))
            .on_action(cx.listener(Self::on_forward))
            .on_action(cx.listener(Self::on_go_home))
            .on_action(cx.listener(Self::on_reload))
            .on_action(cx.listener(Self::on_toggle_hidden))
            .on_action(cx.listener(Self::on_reveal))
            .on_action(cx.listener(Self::on_copy_path))
            .on_action(cx.listener(Self::on_quick_look))
            .on_action(cx.listener(Self::on_stage))
            .on_action(cx.listener(Self::on_empty_queue))
            .on_action(cx.listener(Self::on_palette_up))
            .on_action(cx.listener(Self::on_palette_down))
            .on_action(cx.listener(Self::on_trash))
            .on_action(cx.listener(Self::on_new_folder))
            .on_action(cx.listener(Self::on_rename))
            .on_action(cx.listener(Self::on_duplicate))
            .on_action(cx.listener(Self::on_copy_items))
            .on_action(cx.listener(Self::on_cut_items))
            .on_action(cx.listener(Self::on_paste_items))
            .on_action(cx.listener(Self::on_move_items_here))
            .on_action(cx.listener(Self::on_undo))
            .on_action(cx.listener(Self::on_redo))
            .on_mouse_move(cx.listener(Self::on_drag_move))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|ws, _: &MouseUpEvent, _, cx| ws.end_drag(cx)),
            )
            .when(dragging, |d| d.cursor(CursorStyle::ResizeLeftRight))
            .relative()
            .size_full()
            .bg(t.bg)
            .text_color(t.fg)
            .text_size(FONT_BODY)
            .font_family(cx.theme().font_family.clone())
            .child(self.render_title_bar(window, cx))
            .child(
                h_flex()
                    .flex_1()
                    .min_h_0()
                    .w_full()
                    .when(sidebar_open, |d| {
                        d.child(
                            div()
                                .relative()
                                .flex_none()
                                .h_full()
                                .w(sidebar_w)
                                .child(self.render_sidebar(cx))
                                .child(
                                    self.resize_handle("sidebar-edge", Drag::Sidebar, cx)
                                        .right(-RESIZE_HANDLE / 2.),
                                ),
                        )
                    })
                    .child(div().flex_1().min_w_0().h_full().child(main)),
            )
            .child(self.render_status_bar(cx))
            .children(self.render_palette(window, cx))
            .children(self.render_sheet(cx))
    }
}
