//! Right-click menus for items and for empty space in a view. Built when the
//! menu opens, from the selection at that moment.

use gpui_kit::assets::IconName;
use gpui_kit::component::Icon;
use gpui_kit::component::menu::{PopupMenu, PopupMenuItem};
use gpui_kit::prelude::*;
use gpui_kit::*;

use super::Workspace;
use crate::actions::*;
use crate::tab::is_folder;

pub(crate) const REVEAL_LABEL: &str = if cfg!(target_os = "macos") {
    "Reveal in Finder"
} else {
    "Show in File Manager"
};

/// A menu item that runs `f` on the workspace. `shortcut` names the action
/// whose key binding the menu shows; the click runs `f` either way.
fn item(
    label: &str,
    icon: Option<IconName>,
    shortcut: Option<Box<dyn Action>>,
    ws: &WeakEntity<Workspace>,
    f: impl Fn(&mut Workspace, &mut Window, &mut Context<Workspace>) + 'static,
) -> PopupMenuItem {
    let ws = ws.clone();
    let mut it = PopupMenuItem::new(label.to_owned()).on_click(move |_, window, cx| {
        let _ = ws.update(cx, |ws, cx| f(ws, window, cx));
    });
    if let Some(action) = shortcut {
        it = it.action(action);
    }
    if let Some(icon) = icon {
        it = it.icon(icon);
    }
    it
}

/// Right-click on one or more selected items.
pub(crate) fn item_menu(
    menu: PopupMenu,
    ws: &WeakEntity<Workspace>,
    window: &mut Window,
    cx: &mut Context<PopupMenu>,
) -> PopupMenu {
    let Some(entity) = ws.upgrade() else {
        return menu;
    };
    let w = entity.read(cx);
    let tab = w.tab();
    let selected = tab.selected();
    let count = selected.len();
    if count == 0 {
        return background_menu(menu, ws, cx);
    }
    let single_folder = match selected.as_slice() {
        [only] if is_folder(only) => Some(only.path.clone()),
        _ => None,
    };
    let pinned = single_folder
        .as_ref()
        .is_some_and(|p| w.settings.is_pinned(p));
    let can_paste = w.ops.can_paste();
    let paths = tab.selected_paths();

    let mut menu = menu.item(item(
        "Open",
        Some(IconName::FolderOpen),
        None,
        ws,
        |ws, _, cx| ws.open_selected(cx),
    ));
    if let Some(path) = single_folder.clone() {
        menu = menu.item(item(
            "Open in New Tab",
            Some(IconName::Plus),
            None,
            ws,
            move |ws, _, cx| ws.new_tab_at(path.clone(), cx),
        ));
    }
    if cfg!(target_os = "macos") {
        let apps = af_mac::apps_for(&paths);
        let ws = ws.clone();
        menu = menu.submenu_with_icon(
            Some(Icon::new(IconName::AppWindow)),
            "Open With",
            window,
            cx,
            move |sub, _, _| open_with_menu(sub, &ws, &paths, &apps),
        );
    }
    menu = menu
        .item(item(
            "Quick Look",
            Some(IconName::Eye),
            Some(Box::new(QuickLook)),
            ws,
            |ws, _, cx| ws.quick_look_selected(cx),
        ))
        .item(item(
            REVEAL_LABEL,
            Some(IconName::ExternalLink),
            Some(Box::new(Reveal)),
            ws,
            |ws, _, cx| {
                if let Some(p) = ws.tab().selected_paths().first() {
                    cx.reveal_path(p);
                }
            },
        ))
        .separator()
        .item(item(
            "Copy",
            Some(IconName::Clipboard),
            Some(Box::new(CopyItems)),
            ws,
            |ws, window, cx| ws.copy_selection(false, window, cx),
        ))
        .item(item(
            "Cut",
            Some(IconName::Scissors),
            Some(Box::new(CutItems)),
            ws,
            |ws, window, cx| ws.copy_selection(true, window, cx),
        ))
        .item(item(
            "Copy Path",
            Some(IconName::Copy),
            Some(Box::new(CopyPath)),
            ws,
            |ws, _, cx| {
                let paths = ws.tab().selected_paths();
                ws.copy_paths(&paths, cx)
            },
        ));
    if can_paste {
        menu = menu.item(item(
            "Paste",
            Some(IconName::ClipboardPaste),
            Some(Box::new(PasteItems)),
            ws,
            |ws, window, cx| ws.paste(false, window, cx),
        ));
    }
    menu = menu.separator();
    if count == 1 {
        menu = menu.item(item(
            "Rename\u{2026}",
            Some(IconName::Pencil),
            Some(Box::new(Rename)),
            ws,
            |ws, window, cx| ws.begin_rename(window, cx),
        ));
    }
    menu = menu
        .item(item(
            "Duplicate",
            Some(IconName::CopyPlus),
            Some(Box::new(Duplicate)),
            ws,
            |ws, window, cx| ws.duplicate(window, cx),
        ))
        .item(item(
            "New Folder",
            Some(IconName::FolderPlus),
            Some(Box::new(NewFolder)),
            ws,
            |ws, window, cx| ws.new_folder(window, cx),
        ));
    if let Some(path) = single_folder {
        menu = menu.item(item(
            if pinned {
                "Unpin from Sidebar"
            } else {
                "Pin to Sidebar"
            },
            Some(if pinned {
                IconName::PinOff
            } else {
                IconName::Pin
            }),
            None,
            ws,
            move |ws, _, cx| ws.toggle_pin(&path, cx),
        ));
    }
    menu.item(item(
        "Get Info",
        Some(IconName::Info),
        Some(Box::new(GetInfo)),
        ws,
        |ws, _, cx| {
            if !ws.settings.show_preview {
                ws.toggle_preview_panel(cx);
            }
        },
    ))
    .separator()
    .item(item(
        "Move to Trash",
        Some(IconName::Trash),
        Some(Box::new(MoveToTrash)),
        ws,
        |ws, window, cx| ws.trash_selected(window, cx),
    ))
}

/// The Open With submenu: the apps that take every selected item, the
/// default first, then "Other…" to pick any app.
fn open_with_menu(
    mut menu: PopupMenu,
    ws: &WeakEntity<Workspace>,
    paths: &[std::path::PathBuf],
    apps: &[af_mac::OpenApp],
) -> PopupMenu {
    for (i, app) in apps.iter().enumerate() {
        let label = if app.is_default {
            format!("{} (default)", app.name)
        } else {
            app.name.clone()
        };
        let (paths, app_path) = (paths.to_vec(), app.path.clone());
        menu = menu.item(item(&label, None, None, ws, move |ws, _, cx| {
            ws.open_with(&paths, &app_path, cx)
        }));
        if app.is_default && i + 1 < apps.len() {
            menu = menu.separator();
        }
    }
    if apps.is_empty() {
        menu = menu.item(PopupMenuItem::new("No Apps Found").disabled(true));
    }
    let paths = paths.to_vec();
    menu.separator()
        .item(item("Other\u{2026}", None, None, ws, move |ws, _, cx| {
            ws.open_with_other(paths.clone(), cx)
        }))
}

/// Right-click on empty space: about the folder itself.
pub(crate) fn background_menu(menu: PopupMenu, ws: &WeakEntity<Workspace>, cx: &App) -> PopupMenu {
    let Some(entity) = ws.upgrade() else {
        return menu;
    };
    let w = entity.read(cx);
    let cwd = w.tab().cwd.clone();
    let pinned = w.settings.is_pinned(&cwd);
    let hidden = w.show_hidden;
    let can_paste = w.ops.can_paste();
    let mut menu = menu.item(item(
        "New Folder",
        Some(IconName::FolderPlus),
        Some(Box::new(NewFolder)),
        ws,
        |ws, window, cx| ws.new_folder(window, cx),
    ));
    if can_paste {
        menu = menu.item(item(
            "Paste",
            Some(IconName::ClipboardPaste),
            Some(Box::new(PasteItems)),
            ws,
            |ws, window, cx| ws.paste(false, window, cx),
        ));
    }
    let (tab_path, pin_path, copy_path, reveal_path) = (cwd.clone(), cwd.clone(), cwd.clone(), cwd);
    menu.separator()
        .item(item(
            "Open in New Tab",
            Some(IconName::Plus),
            None,
            ws,
            move |ws, _, cx| ws.new_tab_at(tab_path.clone(), cx),
        ))
        .item(item(
            REVEAL_LABEL,
            Some(IconName::ExternalLink),
            None,
            ws,
            move |_, _, cx| cx.reveal_path(&reveal_path),
        ))
        .item(item(
            "Copy Path",
            Some(IconName::Copy),
            None,
            ws,
            move |ws, _, cx| ws.copy_paths(std::slice::from_ref(&copy_path), cx),
        ))
        .item(item(
            if pinned {
                "Unpin from Sidebar"
            } else {
                "Pin to Sidebar"
            },
            Some(if pinned {
                IconName::PinOff
            } else {
                IconName::Pin
            }),
            None,
            ws,
            move |ws, _, cx| ws.toggle_pin(&pin_path, cx),
        ))
        .separator()
        .item(
            item(
                "Show Hidden Files",
                Some(if hidden {
                    IconName::EyeOff
                } else {
                    IconName::Eye
                }),
                Some(Box::new(ToggleHidden)),
                ws,
                |ws, _, cx| ws.toggle_hidden_files(cx),
            )
            .checked(hidden),
        )
        .item(item(
            "Reload",
            Some(IconName::RefreshCw),
            Some(Box::new(Reload)),
            ws,
            |ws, _, cx| ws.reload(cx),
        ))
        .item(item(
            "Get Info",
            Some(IconName::Info),
            Some(Box::new(GetInfo)),
            ws,
            |ws, _, cx| {
                if !ws.settings.show_preview {
                    ws.toggle_preview_panel(cx);
                }
            },
        ))
}
