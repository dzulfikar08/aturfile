//! aturfile: a fast file manager with disk cleanup.

mod browser;
mod cleanup;
mod theme;

use gpui::{
    App, AppContext as _, Application, Bounds, KeyBinding, Menu, MenuItem, TitlebarOptions,
    WindowBounds, WindowOptions, actions, point, px, size,
};

actions!(
    aturfile,
    [
        Quit,
        SelectNext,
        SelectPrev,
        SelectFirst,
        SelectLast,
        PageDown,
        PageUp,
        Open,
        Parent,
        Back,
        Forward,
        ToggleHidden,
        MoveToTrash,
        Reveal,
        ShowBrowser,
        ShowCleanup,
        Rescan,
        StageSelected,
        EmptyQueue,
    ]
);

fn key_bindings() -> Vec<KeyBinding> {
    let ctx = Some("Workspace");
    vec![
        KeyBinding::new("secondary-q", Quit, None),
        KeyBinding::new("down", SelectNext, ctx),
        KeyBinding::new("up", SelectPrev, ctx),
        KeyBinding::new("home", SelectFirst, ctx),
        KeyBinding::new("end", SelectLast, ctx),
        KeyBinding::new("alt-up", SelectFirst, ctx),
        KeyBinding::new("alt-down", SelectLast, ctx),
        KeyBinding::new("pagedown", PageDown, ctx),
        KeyBinding::new("pageup", PageUp, ctx),
        KeyBinding::new("enter", Open, ctx),
        KeyBinding::new("secondary-down", Open, ctx),
        KeyBinding::new("secondary-o", Open, ctx),
        KeyBinding::new("secondary-up", Parent, ctx),
        KeyBinding::new("secondary-[", Back, ctx),
        KeyBinding::new("secondary-]", Forward, ctx),
        KeyBinding::new("alt-left", Back, ctx),
        KeyBinding::new("alt-right", Forward, ctx),
        KeyBinding::new("secondary-shift-.", ToggleHidden, ctx),
        KeyBinding::new("secondary-backspace", MoveToTrash, ctx),
        KeyBinding::new("secondary-r", Reveal, ctx),
        KeyBinding::new("secondary-1", ShowBrowser, ctx),
        KeyBinding::new("secondary-2", ShowCleanup, ctx),
        KeyBinding::new("secondary-shift-r", Rescan, ctx),
        KeyBinding::new("secondary-s", StageSelected, ctx),
        KeyBinding::new("secondary-shift-backspace", EmptyQueue, ctx),
    ]
}

fn main() {
    // `aturfile [--cleanup] [folder]`
    let mut mode = browser::Mode::Files;
    let mut start = None;
    for arg in std::env::args().skip(1) {
        if arg == "--cleanup" {
            mode = browser::Mode::Cleanup;
        } else {
            start = Some(std::path::PathBuf::from(arg));
        }
    }
    let start = start.unwrap_or_else(af_clean::paths::home);

    Application::new().run(move |cx: &mut App| {
        cx.bind_keys(key_bindings());
        cx.on_action(|_: &Quit, cx| cx.quit());
        cx.set_menus(vec![
            Menu {
                name: "aturfile".into(),
                items: vec![MenuItem::action("Quit aturfile", Quit)],
            },
            Menu {
                name: "Go".into(),
                items: vec![
                    MenuItem::action("Back", Back),
                    MenuItem::action("Forward", Forward),
                    MenuItem::action("Enclosing Folder", Parent),
                    MenuItem::separator(),
                    MenuItem::action("Files", ShowBrowser),
                    MenuItem::action("Clean Up", ShowCleanup),
                ],
            },
            Menu {
                name: "View".into(),
                items: vec![MenuItem::action("Show Hidden Files", ToggleHidden)],
            },
        ]);

        let bounds = Bounds::centered(None, size(px(1180.0), px(760.0)), cx);
        let options = WindowOptions {
            window_bounds: Some(WindowBounds::Windowed(bounds)),
            titlebar: Some(TitlebarOptions {
                title: Some("aturfile".into()),
                appears_transparent: true,
                traffic_light_position: Some(point(px(14.0), px(14.0))),
            }),
            window_min_size: Some(size(px(720.0), px(420.0))),
            app_id: Some("com.atur.aturfile".into()),
            ..WindowOptions::default()
        };
        let opened = cx.open_window(options, |window, cx| {
            cx.new(|cx| browser::Workspace::new(start, mode, window, cx))
        });
        if let Err(error) = opened {
            eprintln!("aturfile: could not open a window: {error}");
            std::process::exit(1);
        }
        cx.on_window_closed(|cx| {
            if cx.windows().is_empty() {
                cx.quit();
            }
        })
        .detach();
        cx.activate(true);
    });
}
