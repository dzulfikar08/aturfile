//! The sidebar: Favorites (standard folders plus pins), Locations (mounted
//! volumes with a capacity bar) and Tools.

use std::path::PathBuf;

use af_ui::components::{meter, section_label};
use af_ui::metrics::*;
use af_ui::tokens;
use gpui_kit::assets::IconName;
use gpui_kit::component::menu::{ContextMenuExt as _, PopupMenu, PopupMenuItem};
use gpui_kit::component::{Icon, v_flex};
use gpui_kit::*;

use super::Workspace;
use crate::appearance;
use crate::tab::Page;

struct Place {
    label: String,
    path: PathBuf,
    icon: IconName,
    pinned: bool,
}

impl Workspace {
    fn places(&self) -> Vec<Place> {
        let home = af_clean::paths::home();
        let std_places = [
            ("Home", home.clone(), IconName::House),
            ("Desktop", home.join("Desktop"), IconName::Monitor),
            ("Documents", home.join("Documents"), IconName::FileText),
            ("Downloads", home.join("Downloads"), IconName::Download),
            (
                "Applications",
                PathBuf::from("/Applications"),
                IconName::AppWindow,
            ),
        ];
        let mut out: Vec<Place> = std_places
            .into_iter()
            .filter(|(_, p, _)| p.is_dir())
            .map(|(label, path, icon)| Place {
                label: label.into(),
                path,
                icon,
                pinned: false,
            })
            .collect();
        for pin in &self.settings.pins {
            if out.iter().any(|p| &p.path == pin) {
                continue;
            }
            out.push(Place {
                label: crate::tab::folder_name(pin),
                path: pin.clone(),
                icon: IconName::Folder,
                pinned: true,
            });
        }
        out
    }

    fn sidebar_row(&self, id: impl Into<ElementId>, active: bool, cx: &App) -> Stateful<Div> {
        let t = tokens(cx);
        div()
            .id(id)
            .flex_none()
            .h(appearance::density(cx).sidebar_row())
            .px(SPACE_4)
            .flex()
            .items_center()
            .gap(SPACE_4)
            .rounded(RADIUS_MD)
            .text_size(FONT_SMALL)
            .cursor_pointer()
            .when_else(
                active,
                |d| d.bg(t.selected).text_color(t.fg),
                |d| {
                    d.text_color(t.fg_muted)
                        .hover(|s| s.bg(t.hover).text_color(t.fg))
                },
            )
    }

    pub(super) fn render_sidebar(&mut self, cx: &mut Context<Self>) -> impl IntoElement {
        let t = tokens(cx);
        let ws = cx.entity().downgrade();
        let tab = self.tab();
        let on_files = tab.page == Page::Files;
        let cwd = tab.cwd.clone();

        let mut favorites: Vec<AnyElement> = Vec::new();
        for (i, place) in self.places().into_iter().enumerate() {
            let active = on_files && cwd == place.path;
            let target = place.path.clone();
            let menu_path = place.path.clone();
            let menu_ws = ws.clone();
            let pinned = place.pinned;
            favorites.push(
                self.sidebar_row(("place", i), active, cx)
                    .child(Icon::new(place.icon).size(ICON_MD).text_color(if active {
                        t.accent
                    } else {
                        t.kind_folder
                    }))
                    .child(div().flex_1().min_w_0().truncate().child(place.label))
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.go(target.clone(), None, cx);
                        ws.focus_list(window, cx);
                    }))
                    .context_menu(move |menu: PopupMenu, _, _| {
                        place_menu(menu, &menu_ws, menu_path.clone(), pinned)
                    })
                    .into_any_element(),
            );
        }

        let mut locations: Vec<AnyElement> = Vec::new();
        for (i, vol) in self.volumes.iter().enumerate() {
            let active = on_files && cwd == vol.path;
            let used = 1.0 - vol.free as f32 / vol.total.max(1) as f32;
            let fill = if used > 0.9 { t.warning } else { t.bar_fill };
            let target = vol.path.clone();
            let menu_path = vol.path.clone();
            let menu_ws = ws.clone();
            locations.push(
                self.sidebar_row(("volume", i), active, cx)
                    .h(px(44.))
                    .child(
                        Icon::new(if vol.is_root {
                            IconName::HardDrive
                        } else {
                            IconName::Usb
                        })
                        .size(ICON_MD)
                        .text_color(if active {
                            t.accent
                        } else {
                            t.fg_muted
                        }),
                    )
                    .child(
                        v_flex()
                            .flex_1()
                            .min_w_0()
                            .gap(SPACE_2)
                            .child(div().truncate().child(vol.name.clone()))
                            .child(meter(used, fill, t.bar_track, px(3.)))
                            .child(
                                div()
                                    .text_size(FONT_CAPTION)
                                    .text_color(t.fg_dim)
                                    .truncate()
                                    .child(format!(
                                        "{} free of {}",
                                        af_fs::format_size(vol.free),
                                        af_fs::format_size(vol.total)
                                    )),
                            ),
                    )
                    .on_click(cx.listener(move |ws, _: &ClickEvent, window, cx| {
                        ws.go(target.clone(), None, cx);
                        ws.focus_list(window, cx);
                    }))
                    .context_menu(move |menu: PopupMenu, _, _| {
                        place_menu(menu, &menu_ws, menu_path.clone(), false)
                    })
                    .into_any_element(),
            );
        }

        let cleanup_active = !on_files;
        let section = |label: &'static str, cx: &App| {
            div()
                .px(SPACE_4)
                .pt(SPACE_7)
                .pb(SPACE_2)
                .child(section_label(label, cx))
        };

        v_flex()
            .id("sidebar")
            .size_full()
            .px(SPACE_3)
            .pb(SPACE_6)
            .gap(px(1.))
            .bg(t.chrome)
            .border_r_1()
            .border_color(t.border)
            .overflow_y_scroll()
            .child(section("Favorites", cx).pt(SPACE_5))
            .children(favorites)
            .when(!locations.is_empty(), |d| {
                d.child(section("Locations", cx)).children(locations)
            })
            .child(section("Tools", cx))
            .child(
                self.sidebar_row("cleanup", cleanup_active, cx)
                    .child(
                        Icon::new(IconName::BrushCleaning)
                            .size(ICON_MD)
                            .text_color(if cleanup_active { t.accent } else { t.success }),
                    )
                    .child(div().flex_1().child("Clean Up"))
                    .on_click(cx.listener(|ws, _: &ClickEvent, _, cx| ws.show_cleanup(cx))),
            )
    }
}

/// Right-click on a sidebar place.
fn place_menu(
    menu: PopupMenu,
    ws: &WeakEntity<Workspace>,
    path: PathBuf,
    pinned: bool,
) -> PopupMenu {
    let open_tab = {
        let (ws, path) = (ws.clone(), path.clone());
        PopupMenuItem::new("Open in New Tab")
            .icon(IconName::Plus)
            .on_click(move |_, _, cx| {
                let _ = ws.update(cx, |ws, cx| ws.new_tab_at(path.clone(), cx));
            })
    };
    let reveal = {
        let path = path.clone();
        PopupMenuItem::new(super::menus::REVEAL_LABEL)
            .icon(IconName::ExternalLink)
            .on_click(move |_, _, cx| cx.reveal_path(&path))
    };
    let copy = {
        let (ws, path) = (ws.clone(), path.clone());
        PopupMenuItem::new("Copy Path")
            .icon(IconName::Copy)
            .on_click(move |_, _, cx| {
                let _ = ws.update(cx, |ws, cx| ws.copy_paths(std::slice::from_ref(&path), cx));
            })
    };
    let menu = menu.item(open_tab).item(reveal).item(copy);
    if pinned {
        let ws = ws.clone();
        menu.separator().item(
            PopupMenuItem::new("Unpin from Sidebar")
                .icon(IconName::PinOff)
                .on_click(move |_, _, cx| {
                    let _ = ws.update(cx, |ws, cx| ws.toggle_pin(&path, cx));
                }),
        )
    } else {
        menu
    }
}
