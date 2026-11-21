//! The command palette (Cmd-K): every command by name, plus the theme and
//! density pickers (also Cmd-,).

use af_ui::catalog::{self, ThemeEntry, ThemeId};
use af_ui::components::kbd;
use af_ui::metrics::*;
use af_ui::tokens::{Elevation, tokens};
use gpui_kit::assets::IconName;
use gpui_kit::component::input::{InputEvent, InputState};
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Workspace;
use crate::actions::*;
use crate::appearance;
use crate::tab::ViewMode;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Mode {
    Commands,
    Themes,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Cmd {
    NewTab,
    CloseTab,
    GoHome,
    GoDesktop,
    GoDocuments,
    GoDownloads,
    GoApplications,
    GoToFolder,
    Back,
    Forward,
    Parent,
    Reload,
    ViewList,
    ViewColumns,
    ViewIcons,
    ToggleSidebar,
    TogglePreview,
    ToggleHidden,
    CleanUp,
    Filter,
    NewFolder,
    Rename,
    Duplicate,
    Trash,
    CopyPath,
    Reveal,
    PinFolder,
    Undo,
    Redo,
    ChooseTheme,
    Density(Density),
    Theme(usize),
}

struct Row {
    label: String,
    keys: Option<&'static str>,
    cmd: Cmd,
}

pub(crate) struct PaletteState {
    mode: Mode,
    input: Entity<InputState>,
    rows: Vec<Row>,
    themes: Vec<ThemeEntry>,
    /// Indexes into `rows` that match the query.
    matches: Vec<usize>,
    selected: usize,
    scroll: UniformListScrollHandle,
    _subscription: Subscription,
}

fn command_rows() -> Vec<Row> {
    let r = |label: &str, keys: Option<&'static str>, cmd: Cmd| Row {
        label: label.to_owned(),
        keys,
        cmd,
    };
    vec![
        r("New Tab", Some("Cmd-T"), Cmd::NewTab),
        r("Close Tab", Some("Cmd-W"), Cmd::CloseTab),
        r("Go to Folder\u{2026}", Some("Cmd-L"), Cmd::GoToFolder),
        r("Go to Home", Some("Cmd-Shift-H"), Cmd::GoHome),
        r("Go to Desktop", None, Cmd::GoDesktop),
        r("Go to Documents", None, Cmd::GoDocuments),
        r("Go to Downloads", None, Cmd::GoDownloads),
        r("Go to Applications", None, Cmd::GoApplications),
        r("Back", Some("Cmd-["), Cmd::Back),
        r("Forward", Some("Cmd-]"), Cmd::Forward),
        r("Enclosing Folder", Some("Cmd-Up"), Cmd::Parent),
        r("Reload", Some("Cmd-Shift-R"), Cmd::Reload),
        r("View as List", Some("Cmd-1"), Cmd::ViewList),
        r("View as Columns", Some("Cmd-2"), Cmd::ViewColumns),
        r("View as Icons", Some("Cmd-3"), Cmd::ViewIcons),
        r("Toggle Sidebar", Some("Cmd-Alt-S"), Cmd::ToggleSidebar),
        r("Toggle Preview", Some("Cmd-Alt-P"), Cmd::TogglePreview),
        r(
            "Show or Hide Hidden Files",
            Some("Cmd-Shift-."),
            Cmd::ToggleHidden,
        ),
        r("Filter This Folder", Some("Cmd-F"), Cmd::Filter),
        r("Clean Up", Some("Cmd-Shift-K"), Cmd::CleanUp),
        r("New Folder", Some("Cmd-Shift-N"), Cmd::NewFolder),
        r("Rename\u{2026}", Some("F2"), Cmd::Rename),
        r("Duplicate", Some("Cmd-D"), Cmd::Duplicate),
        r("Move to Trash", Some("Cmd-Backspace"), Cmd::Trash),
        r("Copy Path", Some("Cmd-Alt-C"), Cmd::CopyPath),
        r(super::menus::REVEAL_LABEL, Some("Cmd-R"), Cmd::Reveal),
        r("Pin or Unpin This Folder", None, Cmd::PinFolder),
        r("Undo", Some("Cmd-Z"), Cmd::Undo),
        r("Redo", Some("Cmd-Shift-Z"), Cmd::Redo),
        r("Theme\u{2026}", Some("Cmd-,"), Cmd::ChooseTheme),
        r("Density: Compact", None, Cmd::Density(Density::Compact)),
        r("Density: Airy", None, Cmd::Density(Density::Airy)),
    ]
}

fn theme_rows(themes: &[ThemeEntry]) -> Vec<Row> {
    let mut rows = vec![Row {
        label: "System (aturfile Dark or Light)".into(),
        keys: None,
        cmd: Cmd::Theme(usize::MAX),
    }];
    rows.extend(themes.iter().enumerate().map(|(i, t)| Row {
        label: t.name.clone(),
        keys: None,
        cmd: Cmd::Theme(i),
    }));
    rows
}

/// Rows whose label contains the query's words (case-insensitive), in order.
fn rank(rows: &[Row], query: &str) -> Vec<usize> {
    let words: Vec<String> = query.split_whitespace().map(str::to_lowercase).collect();
    rows.iter()
        .enumerate()
        .filter(|(_, r)| {
            let label = r.label.to_lowercase();
            words.iter().all(|w| label.contains(w.as_str()))
        })
        .map(|(i, _)| i)
        .collect()
}

impl Workspace {
    pub(super) fn open_palette(&mut self, mode: Mode, window: &mut Window, cx: &mut Context<Self>) {
        let placeholder = match mode {
            Mode::Commands => "Type a command",
            Mode::Themes => "Choose a theme",
        };
        let input = cx.new(|cx| InputState::new(window, cx).placeholder(placeholder));
        let subscription = cx.subscribe_in(&input, window, Self::on_palette_input);
        input.update(cx, |i, cx| i.focus(window, cx));
        let themes = catalog::all();
        let rows = match mode {
            Mode::Commands => command_rows(),
            Mode::Themes => theme_rows(&themes),
        };
        let selected = match mode {
            Mode::Themes => match &appearance::current(cx).theme {
                ThemeId::System => 0,
                ThemeId::Named(n) => themes
                    .iter()
                    .position(|t| &t.name == n)
                    .map_or(0, |i| i + 1),
            },
            Mode::Commands => 0,
        };
        let scroll = UniformListScrollHandle::new();
        scroll.scroll_to_item(selected, ScrollStrategy::Center);
        self.palette = Some(PaletteState {
            mode,
            input,
            matches: (0..rows.len()).collect(),
            rows,
            themes,
            selected,
            scroll,
            _subscription: subscription,
        });
        cx.notify();
    }

    pub(super) fn close_palette(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        if self.palette.take().is_some() {
            self.focus_list(window, cx);
            cx.notify();
        }
    }

    pub(super) fn palette_move(&mut self, delta: isize, cx: &mut Context<Self>) {
        let Some(p) = self.palette.as_mut() else {
            return;
        };
        let n = p.matches.len();
        if n == 0 {
            return;
        }
        p.selected = (p.selected as isize + delta).rem_euclid(n as isize) as usize;
        p.scroll.scroll_to_item(p.selected, ScrollStrategy::Nearest);
        cx.notify();
    }

    fn on_palette_input(
        &mut self,
        input: &Entity<InputState>,
        event: &InputEvent,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        match event {
            InputEvent::Change => {
                let query = input.read(cx).value().to_string();
                if let Some(p) = self.palette.as_mut() {
                    p.matches = rank(&p.rows, &query);
                    p.selected = 0;
                    p.scroll.scroll_to_item(0, ScrollStrategy::Top);
                }
                cx.notify();
            }
            InputEvent::PressEnter { .. } => self.run_palette_row(None, window, cx),
            _ => {}
        }
    }

    fn run_palette_row(&mut self, row: Option<usize>, window: &mut Window, cx: &mut Context<Self>) {
        let Some(p) = self.palette.as_ref() else {
            return;
        };
        let pick = row.or_else(|| p.matches.get(p.selected).copied());
        let Some(cmd) = pick.and_then(|i| p.rows.get(i)).map(|r| r.cmd) else {
            return;
        };
        let theme = match cmd {
            Cmd::Theme(usize::MAX) => Some(ThemeId::System),
            Cmd::Theme(i) => p.themes.get(i).map(|t| ThemeId::Named(t.name.clone())),
            _ => None,
        };
        self.close_palette(window, cx);
        if let Some(id) = theme {
            self.settings.theme = id.to_setting();
            self.save_settings();
            appearance::set_theme(id, cx);
            return;
        }
        self.run_command(cmd, window, cx);
    }

    fn run_command(&mut self, cmd: Cmd, window: &mut Window, cx: &mut Context<Self>) {
        let home = af_clean::paths::home();
        match cmd {
            Cmd::NewTab => self.on_new_tab(&NewTab, window, cx),
            Cmd::CloseTab => self.on_close_tab(&CloseTab, window, cx),
            Cmd::GoHome => self.go(home, None, cx),
            Cmd::GoDesktop => self.go(home.join("Desktop"), None, cx),
            Cmd::GoDocuments => self.go(home.join("Documents"), None, cx),
            Cmd::GoDownloads => self.go(home.join("Downloads"), None, cx),
            Cmd::GoApplications => self.go("/Applications".into(), None, cx),
            Cmd::GoToFolder => self.start_editing_location(window, cx),
            Cmd::Back => self.back(cx),
            Cmd::Forward => self.forward(cx),
            Cmd::Parent => self.up(cx),
            Cmd::Reload => self.on_reload(&Reload, window, cx),
            Cmd::ViewList => self.set_view(ViewMode::List, cx),
            Cmd::ViewColumns => self.set_view(ViewMode::Columns, cx),
            Cmd::ViewIcons => self.set_view(ViewMode::Icons, cx),
            Cmd::ToggleSidebar => self.toggle_sidebar_panel(cx),
            Cmd::TogglePreview => self.toggle_preview_panel(cx),
            Cmd::ToggleHidden => self.toggle_hidden_files(cx),
            Cmd::Filter => self.on_focus_filter(&FocusFilter, window, cx),
            Cmd::CleanUp => self.show_cleanup(cx),
            Cmd::NewFolder => self.new_folder(window, cx),
            Cmd::Rename => self.begin_rename(window, cx),
            Cmd::Duplicate => self.duplicate(window, cx),
            Cmd::Trash => self.trash_selected(window, cx),
            Cmd::CopyPath => {
                let paths = self.targets();
                self.copy_paths(&paths, cx)
            }
            Cmd::Reveal => {
                if let Some(p) = self.targets().first() {
                    cx.reveal_path(p);
                }
            }
            Cmd::PinFolder => {
                let cwd = self.tab().cwd.clone();
                self.toggle_pin(&cwd, cx)
            }
            Cmd::Undo => self.undo(window, cx),
            Cmd::Redo => self.redo(window, cx),
            Cmd::ChooseTheme => self.open_palette(Mode::Themes, window, cx),
            Cmd::Density(d) => {
                self.settings.density = d.as_str().into();
                self.save_settings();
                appearance::set_density(d, cx);
                self.set_notice(format!("Density: {}", d.label()), cx);
            }
            Cmd::Theme(_) => {}
        }
    }

    pub(super) fn render_palette(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> Option<AnyElement> {
        let _ = window;
        let t = tokens(cx);
        let p = self.palette.as_ref()?;
        let count = p.matches.len();
        let visible = count.clamp(1, PALETTE_MAX_ROWS) as f32;
        let list = uniform_list(
            "palette-rows",
            count,
            cx.processor(|ws: &mut Self, range: std::ops::Range<usize>, _, cx| {
                ws.palette_rows(range, cx)
            }),
        )
        .track_scroll(&p.scroll)
        .w_full()
        .h(PALETTE_ROW_HEIGHT * visible);
        let title = match p.mode {
            Mode::Commands => "Commands",
            Mode::Themes => "Theme",
        };
        let footer = h_flex()
            .h(PALETTE_FOOTER_HEIGHT)
            .px(SPACE_6)
            .gap(SPACE_6)
            .bg(t.chrome)
            .border_t_1()
            .border_color(t.border)
            .text_size(FONT_CAPTION)
            .text_color(t.fg_muted)
            .child(
                h_flex()
                    .gap(SPACE_2)
                    .child(kbd("Up", cx))
                    .child(kbd("Down", cx))
                    .child("move"),
            )
            .child(h_flex().gap(SPACE_2).child(kbd("Enter", cx)).child("run"))
            .child(h_flex().gap(SPACE_2).child(kbd("Esc", cx)).child("close"))
            .child(div().flex_1())
            .child(match p.mode {
                Mode::Themes => appearance::theme_label(cx),
                Mode::Commands => title.to_string(),
            });
        let panel =
            v_flex()
                .id("palette")
                .key_context(PALETTE)
                .w(PALETTE_WIDTH)
                .mt(PALETTE_TOP)
                .rounded(RADIUS_XL)
                .bg(t.elevated)
                .border_1()
                .border_color(t.border_strong)
                .shadow(t.shadow(Elevation::Modal))
                .overflow_hidden()
                .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                .child(
                    h_flex()
                        .p(SPACE_4)
                        .gap(SPACE_3)
                        .border_b_1()
                        .border_color(t.border)
                        .child(
                            Icon::new(IconName::Command)
                                .size(ICON_MD)
                                .text_color(t.fg_dim),
                        )
                        .child(div().flex_1().child(
                            gpui_kit::component::input::Input::new(&p.input).appearance(false),
                        )),
                )
                .child(div().p(SPACE_3).when_else(
                    count == 0,
                    |d| {
                        d.child(
                            div()
                                .h(PALETTE_ROW_HEIGHT)
                                .px(SPACE_6)
                                .flex()
                                .items_center()
                                .text_size(FONT_SMALL)
                                .text_color(t.fg_dim)
                                .child("No matches"),
                        )
                    },
                    |d| d.child(list),
                ))
                .child(footer);
        Some(
            div()
                .id("palette-scrim")
                .absolute()
                .inset_0()
                .flex()
                .justify_center()
                .items_start()
                .bg(t.scrim)
                .on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|ws, _: &MouseDownEvent, window, cx| ws.close_palette(window, cx)),
                )
                .child(panel)
                .into_any_element(),
        )
    }

    fn palette_rows(
        &mut self,
        range: std::ops::Range<usize>,
        cx: &mut Context<Self>,
    ) -> Vec<AnyElement> {
        let t = tokens(cx);
        let Some(p) = self.palette.as_ref() else {
            return Vec::new();
        };
        let current_theme = appearance::current(cx).theme.clone();
        let density = appearance::density(cx);
        let mut out = Vec::new();
        for i in range {
            let Some(&row_ix) = p.matches.get(i) else {
                continue;
            };
            let row = &p.rows[row_ix];
            let selected = i == p.selected;
            let checked = match row.cmd {
                Cmd::Theme(usize::MAX) => current_theme == ThemeId::System,
                Cmd::Theme(ix) => p
                    .themes
                    .get(ix)
                    .is_some_and(|e| current_theme == ThemeId::Named(e.name.clone())),
                Cmd::Density(d) => d == density,
                _ => false,
            };
            let swatch = match row.cmd {
                Cmd::Theme(ix) if ix != usize::MAX => p.themes.get(ix).map(|e| {
                    let bg = gpui_kit::rgb(e.palette.background);
                    h_flex()
                        .flex_none()
                        .h(px(14.))
                        .rounded(RADIUS_SM)
                        .overflow_hidden()
                        .border_1()
                        .border_color(t.border)
                        .child(div().w(px(14.)).h_full().bg(bg))
                        .children(
                            e.swatch()
                                .into_iter()
                                .map(|c| div().w(px(8.)).h_full().bg(gpui_kit::rgb(c))),
                        )
                }),
                _ => None,
            };
            out.push(
                h_flex()
                    .id(("palette-row", i))
                    .w_full()
                    .h(PALETTE_ROW_HEIGHT)
                    .px(SPACE_5)
                    .gap(SPACE_4)
                    .rounded(RADIUS_MD)
                    .text_size(FONT_BODY)
                    .cursor_pointer()
                    .when_else(
                        selected,
                        |d| d.bg(t.selected).text_color(t.fg),
                        |d| d.text_color(t.fg_muted).hover(|s| s.bg(t.hover)),
                    )
                    .child(div().flex_none().w(px(14.)).when(checked, |d| {
                        d.child(
                            Icon::new(IconName::Check)
                                .size(ICON_SM)
                                .text_color(t.accent),
                        )
                    }))
                    .child(div().flex_1().min_w_0().truncate().child(row.label.clone()))
                    .children(swatch)
                    .children(row.keys.map(|k| kbd(k, cx)))
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.run_palette_row(Some(row_ix), window, cx)
                    }))
                    .into_any_element(),
            );
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[std::prelude::v1::test]
    fn ranks_by_words() {
        let rows = command_rows();
        let hits = rank(&rows, "view col");
        assert_eq!(rows[hits[0]].label, "View as Columns");
        assert_eq!(rank(&rows, "").len(), rows.len());
        assert!(rank(&rows, "zzzz").is_empty());
    }
}
