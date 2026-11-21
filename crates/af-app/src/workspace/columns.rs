//! Miller columns. The folder you are in is the interactive column; the
//! columns to its left are its ancestors (the path component highlighted),
//! and the column to its right shows the selected folder's contents, or a
//! preview when a file is selected. Left goes up, Right goes in.

use std::path::{Path, PathBuf};

use af_fs::Entry;
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::component::menu::ContextMenuExt as _;
use gpui_kit::component::{Icon, h_flex, v_flex};
use gpui_kit::*;

use super::list::file_icon;
use super::{Workspace, menus};
use crate::appearance;
use crate::tab::{Listing, is_folder};

/// A column other than the current folder's.
enum Side {
    /// An ancestor: its child on the path is highlighted.
    Ancestor { highlight: Option<String> },
    /// The selected folder's contents.
    Child,
}

impl Workspace {
    /// Right in the columns view: into the selected folder.
    pub(super) fn columns_enter(&mut self, cx: &mut Context<Self>) {
        let Some(entry) = self.tab().current().cloned() else {
            return;
        };
        if is_folder(&entry) {
            self.go(entry.path, None, cx);
            let tab = self.tab_mut();
            // Land on the first item, like Finder.
            tab.selection = af_fs::Selection::new();
            cx.notify();
        }
    }

    /// Folders from the chain's root (Home, or `/`) down to the current one.
    fn column_chain(&self) -> Vec<PathBuf> {
        let cwd = &self.tab().cwd;
        let home = af_clean::paths::home();
        let root = if cwd.starts_with(&home) {
            home
        } else {
            PathBuf::from("/")
        };
        let mut chain: Vec<PathBuf> = cwd
            .ancestors()
            .take_while(|p| p.starts_with(&root))
            .map(Path::to_path_buf)
            .collect();
        chain.reverse();
        chain
    }

    /// Start listing `dir` for a column unless it is listed or loading.
    fn ensure_listing(&mut self, dir: &Path, cx: &mut Context<Self>) {
        let ix = self.active;
        let tab = &mut self.tabs[ix];
        if tab.listings.contains_key(dir) || tab.listing_tasks.contains_key(dir) {
            return;
        }
        let id = tab.id;
        let read = dir.to_path_buf();
        let key = dir.to_path_buf();
        let work = cx.background_executor().spawn(async move {
            let listed = af_fs::list_dir(&read);
            listed.map(|mut v| {
                af_fs::sort(&mut v, af_fs::SortKey::Name, true);
                v
            })
        });
        let task = cx.spawn(async move |this, cx| {
            let listed = work.await;
            let _ = this.update(cx, |ws, cx| {
                if let Some(tab) = ws.tabs.iter_mut().find(|t| t.id == id) {
                    tab.listing_tasks.remove(&key);
                    let listing = match listed {
                        Ok(entries) => Listing {
                            entries,
                            error: None,
                        },
                        Err(e) => Listing {
                            entries: Vec::new(),
                            error: Some(e.to_string().into()),
                        },
                    };
                    tab.listings.insert(key, listing);
                    cx.notify();
                }
            });
        });
        tab.listing_tasks.insert(dir.to_path_buf(), task);
    }

    pub(super) fn render_columns(
        &mut self,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = tokens(cx);
        let mut avail = f32::from(window.viewport_size().width);
        if self.settings.show_sidebar {
            avail -= self.settings.sidebar_width;
        }
        let fit = ((avail / COLUMN_WIDTH).floor() as usize).max(2);

        let chain = self.column_chain();
        let current = self.tab().current().cloned();
        let trailing_folder = current
            .as_ref()
            .filter(|e| is_folder(e))
            .map(|e| e.path.clone());
        let trailing_file = current.filter(|e| !is_folder(e));
        let total = chain.len() + 1;
        let skip = total.saturating_sub(fit);

        // Load what will be shown.
        for dir in chain.iter().take(chain.len().saturating_sub(1)).skip(skip) {
            self.ensure_listing(dir, cx);
        }
        if let Some(dir) = &trailing_folder {
            self.ensure_listing(dir, cx);
        }

        let mut cols: Vec<AnyElement> = Vec::new();
        let last_chain = chain.len().saturating_sub(1);
        for (i, dir) in chain.iter().enumerate().skip(skip) {
            if i == last_chain {
                cols.push(self.current_column(cx).into_any_element());
            } else {
                let highlight = chain
                    .get(i + 1)
                    .and_then(|p| p.file_name())
                    .map(|n| n.to_string_lossy().into_owned());
                cols.push(
                    self.side_column(("ancestor", i), dir, Side::Ancestor { highlight }, cx)
                        .into_any_element(),
                );
            }
        }
        if let Some(dir) = trailing_folder {
            cols.push(
                self.side_column("child", &dir, Side::Child, cx)
                    .into_any_element(),
            );
        } else if let Some(entry) = trailing_file {
            self.sync_preview(cx);
            cols.push(
                div()
                    .flex_1()
                    .min_w(px(COLUMN_WIDTH))
                    .h_full()
                    .bg(t.chrome)
                    .child(self.render_preview_for(&entry, true, cx))
                    .into_any_element(),
            );
        }
        h_flex()
            .id("columns")
            .size_full()
            .overflow_hidden()
            .children(cols)
            .child(div().flex_1())
    }

    fn column_frame(id: impl Into<ElementId>, cx: &App) -> Stateful<Div> {
        let t = tokens(cx);
        v_flex()
            .id(id)
            .flex_none()
            .w(px(COLUMN_WIDTH))
            .h_full()
            .border_r_1()
            .border_color(t.border)
    }

    /// The current folder: selection, keyboard, context menus.
    fn current_column(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let ws = cx.entity().downgrade();
        let tab = self.tab();
        let body = if tab.visible.is_empty() {
            div()
                .flex_1()
                .flex()
                .items_center()
                .justify_center()
                .text_size(FONT_SMALL)
                .text_color(t.fg_dim)
                .child(if tab.loaded { "Empty folder" } else { "" })
                .into_any_element()
        } else {
            uniform_list(
                "column-current",
                tab.visible.len(),
                cx.processor(|ws, range: std::ops::Range<usize>, _, cx| {
                    range
                        .filter_map(|row| {
                            let tab = ws.tab();
                            let entry = tab.entry_at(row)?.clone();
                            let selected = tab.selection.contains(row);
                            Some(
                                ws.column_row(&entry, selected, true, cx)
                                    .id(("cur", row))
                                    .on_mouse_down(
                                        MouseButton::Left,
                                        cx.listener(move |ws, e: &MouseDownEvent, window, cx| {
                                            if e.click_count == 1 {
                                                ws.click_row(row, e.modifiers, window, cx);
                                            }
                                        }),
                                    )
                                    .on_mouse_down(
                                        MouseButton::Right,
                                        cx.listener(move |ws, _: &MouseDownEvent, _, cx| {
                                            ws.right_click_row(row, cx)
                                        }),
                                    )
                                    .on_click(cx.listener(move |ws, e: &ClickEvent, _, cx| {
                                        cx.stop_propagation();
                                        if e.click_count() >= 2 {
                                            ws.open_row(row, cx);
                                        }
                                    }))
                                    .context_menu({
                                        let ws = cx.entity().downgrade();
                                        move |menu, _, cx| menus::item_menu(menu, &ws, cx)
                                    })
                                    .into_any_element(),
                            )
                        })
                        .collect::<Vec<_>>()
                }),
            )
            .track_scroll(&tab.scroll)
            .flex_1()
            .py(SPACE_2)
            .into_any_element()
        };
        Self::column_frame("column-current-frame", cx)
            .bg(t.bg)
            .on_mouse_down(
                MouseButton::Right,
                cx.listener(|ws, _: &MouseDownEvent, _, cx| ws.right_click_background(cx)),
            )
            .child(body)
            .context_menu(move |menu, _, cx| menus::background_menu(menu, &ws, cx))
    }

    /// An ancestor or child column (read-only; a click navigates).
    fn side_column(
        &mut self,
        id: impl Into<ElementId>,
        dir: &Path,
        side: Side,
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        let t = tokens(cx);
        let show_hidden = self.show_hidden;
        let listing = self.tab().listings.get(dir);
        let entries: Vec<Entry> = listing
            .map(|l| {
                l.entries
                    .iter()
                    .filter(|e| show_hidden || !e.hidden)
                    .cloned()
                    .collect()
            })
            .unwrap_or_default();
        let error = listing.and_then(|l| l.error.clone());
        let mut rows: Vec<AnyElement> = Vec::with_capacity(entries.len().min(400));
        let highlight = match &side {
            Side::Ancestor { highlight } => highlight.clone(),
            Side::Child => None,
        };
        // Side columns show at most a screenful-and-some of rows; ancestors
        // keep the highlighted one in range.
        let hl_ix = highlight
            .as_ref()
            .and_then(|h| entries.iter().position(|e| &e.name == h))
            .unwrap_or(0);
        let start = hl_ix.saturating_sub(150);
        for (i, entry) in entries.iter().enumerate().skip(start).take(400) {
            let selected = highlight.as_deref() == Some(entry.name.as_str());
            let dir_path = dir.to_path_buf();
            let name = entry.name.clone();
            let child_path = entry.path.clone();
            let folder = is_folder(entry);
            rows.push(
                self.column_row(entry, selected, false, cx)
                    .id(("side", i))
                    .on_click(cx.listener(move |ws, e: &ClickEvent, window, cx| {
                        if e.click_count() >= 2 && !folder {
                            cx.open_with_system(&child_path);
                            return;
                        }
                        // Go to the folder this column lists, selecting the item.
                        ws.go(dir_path.clone(), Some(name.clone()), cx);
                        ws.focus_list(window, cx);
                    }))
                    .into_any_element(),
            );
        }
        let mut body = v_flex()
            .id("side-body")
            .flex_1()
            .min_h_0()
            .py(SPACE_2)
            .overflow_y_scroll();
        if rows.is_empty() {
            body = body.child(
                div()
                    .pt(SPACE_9)
                    .text_center()
                    .text_size(FONT_SMALL)
                    .text_color(if error.is_some() { t.danger } else { t.fg_dim })
                    .child(match error {
                        Some(e) => SharedString::from(e),
                        None if listing.is_some() => "Empty folder".into(),
                        None => SharedString::default(),
                    }),
            );
        }
        Self::column_frame(id, cx)
            .bg(match side {
                Side::Ancestor { .. } => t.chrome,
                Side::Child => t.bg,
            })
            .child(body.children(rows))
    }

    fn column_row(&self, entry: &Entry, selected: bool, active: bool, cx: &App) -> Div {
        let t = tokens(cx);
        let (icon, color) = file_icon(entry, &t);
        let row_h = appearance::density(cx).row_height();
        div().w_full().h(row_h).px(SPACE_3).flex_none().child(
            h_flex()
                .size_full()
                .px(SPACE_3)
                .gap(SPACE_4)
                .rounded(RADIUS_MD)
                .when_else(
                    selected,
                    |d| {
                        d.bg(if active {
                            t.selected
                        } else {
                            t.selected_inactive
                        })
                    },
                    |d| d.hover(|s| s.bg(t.hover)),
                )
                .when(entry.hidden, |d| d.opacity(0.6))
                .child(Icon::new(icon).size(ICON_MD).text_color(color).flex_none())
                .child(
                    div()
                        .flex_1()
                        .min_w_0()
                        .truncate()
                        .child(entry.name.clone()),
                )
                .when(is_folder(entry), |d| {
                    d.child(
                        Icon::new(gpui_kit::assets::IconName::ChevronRight)
                            .size(ICON_SM)
                            .text_color(t.fg_dim)
                            .flex_none(),
                    )
                }),
        )
    }
}
