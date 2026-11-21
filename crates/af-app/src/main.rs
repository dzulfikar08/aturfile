//! aturfile: a fast file manager with disk cleanup.
//!
//! ```text
//! aturfile [folder]
//!     --cleanup              open on Clean Up
//!     --view list|columns|icons
//!     --preview              show the preview pane
//!     --select NAME          select NAME in the folder once it is listed
//!     --theme NAME           use a bundled theme for this run ("system" follows macOS)
//!     --density compact|airy
//!     --palette              open the command palette
//!     --window WxH           window size in points
//! ```

extern crate gpui_kit as gpui;

mod actions;
mod appearance;
mod cleanup;
mod format;
mod settings;
mod tab;
mod workspace;

use af_ui::catalog::ThemeId;
use af_ui::metrics::Density;
use gpui_kit::component::TitleBar;
use gpui_kit::*;

use crate::actions::*;
use crate::settings::Settings;
use crate::tab::ViewMode;
use crate::workspace::{StartOptions, Workspace};

struct Args {
    start: StartOptions,
    theme: Option<String>,
    density: Option<String>,
    window: Option<(f32, f32)>,
}

fn parse_args() -> Args {
    let mut args = Args {
        start: StartOptions::default(),
        theme: None,
        density: None,
        window: None,
    };
    let mut dir = None;
    let mut it = std::env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--cleanup" => args.start.cleanup = true,
            "--preview" => args.start.preview = true,
            "--palette" => args.start.palette = true,
            "--view" => args.start.view = it.next().map(|v| ViewMode::parse(&v)),
            "--select" => args.start.select = it.next(),
            "--theme" => args.theme = it.next(),
            "--density" => args.density = it.next(),
            "--window" => {
                args.window = it.next().and_then(|v| {
                    let (w, h) = v.split_once('x')?;
                    Some((w.parse().ok()?, h.parse().ok()?))
                });
            }
            _ => dir = Some(std::path::PathBuf::from(arg)),
        }
    }
    args.start.dir = dir
        .map(|d| std::path::absolute(&d).unwrap_or(d))
        .unwrap_or_else(af_clean::paths::home);
    args
}

fn menus() -> Vec<Menu> {
    vec![
        Menu::new("aturfile").items([
            MenuItem::action("Settings\u{2026}", OpenSettings),
            MenuItem::separator(),
            MenuItem::action("Quit aturfile", Quit),
        ]),
        Menu::new("File").items([
            MenuItem::action("New Tab", NewTab),
            MenuItem::action("New Folder", NewFolder),
            MenuItem::separator(),
            MenuItem::action("Open", Open),
            MenuItem::action("Quick Look", QuickLook),
            MenuItem::action("Rename\u{2026}", Rename),
            MenuItem::action("Duplicate", Duplicate),
            MenuItem::action("Move to Trash", MoveToTrash),
            MenuItem::separator(),
            MenuItem::action("Close Tab", CloseTab),
        ]),
        Menu::new("Edit").items([
            MenuItem::action("Undo", Undo),
            MenuItem::action("Redo", Redo),
            MenuItem::separator(),
            MenuItem::action("Cut", CutItems),
            MenuItem::action("Copy", CopyItems),
            MenuItem::action("Paste", PasteItems),
            MenuItem::action("Move Item Here", MoveItemsHere),
            MenuItem::action("Copy Path", CopyPath),
            MenuItem::action("Select All", SelectAll),
            MenuItem::separator(),
            MenuItem::action("Filter", FocusFilter),
        ]),
        Menu::new("View").items([
            MenuItem::action("as List", ShowList),
            MenuItem::action("as Columns", ShowColumns),
            MenuItem::action("as Icons", ShowIcons),
            MenuItem::separator(),
            MenuItem::action("Show Sidebar", ToggleSidebar),
            MenuItem::action("Show Preview", TogglePreview),
            MenuItem::action("Show Hidden Files", ToggleHidden),
            MenuItem::separator(),
            MenuItem::action("Commands\u{2026}", TogglePalette),
        ]),
        Menu::new("Go").items([
            MenuItem::action("Back", Back),
            MenuItem::action("Forward", Forward),
            MenuItem::action("Enclosing Folder", Parent),
            MenuItem::action("Home", GoHome),
            MenuItem::action("Go to Folder\u{2026}", EditLocation),
            MenuItem::separator(),
            MenuItem::action("Next Tab", NextTab),
            MenuItem::action("Previous Tab", PrevTab),
            MenuItem::separator(),
            MenuItem::action("Clean Up", ShowCleanup),
        ]),
    ]
}

fn main() {
    let args = parse_args();
    let settings = Settings::load();
    let theme = ThemeId::parse(args.theme.as_deref().unwrap_or(&settings.theme));
    let density = Density::parse(args.density.as_deref().unwrap_or(&settings.density));
    let window_size = args.window;
    let start = args.start;

    gpui_kit::application()
        .with_assets(af_ui::AppAssets)
        .run(move |cx: &mut App| {
            gpui_kit::init(cx);
            appearance::init(theme, density, cx);
            actions::bind_keys(cx);
            cx.on_action(|_: &Quit, cx| cx.quit());
            cx.set_menus(menus());

            // Never ask for more than the screen: macOS would shrink the window
            // before it is on a screen, which can leave GPUI drawing at the wrong scale.
            let (w, h) = window_size.unwrap_or_else(|| {
                let screen = cx
                    .primary_display()
                    .map(|d| d.bounds().size)
                    .unwrap_or(size(px(1440.0), px(900.0)));
                (
                    1240.0_f32.min(f32::from(screen.width) * 0.92),
                    780.0_f32.min(f32::from(screen.height) * 0.85),
                )
            });
            let bounds = Bounds::centered(None, size(px(w), px(h)), cx);
            let mut titlebar = TitleBar::title_bar_options();
            titlebar.title = Some("aturfile".into());
            titlebar.traffic_light_position = Some(point(px(13.0), px(13.0)));
            let options = WindowOptions {
                window_bounds: Some(WindowBounds::Windowed(bounds)),
                titlebar: Some(titlebar),
                window_min_size: Some(size(px(760.0), px(440.0))),
                app_id: Some("com.atur.aturfile".into()),
                ..TitleBar::window_options()
            };
            let opened = gpui_kit::open_window(options, cx, move |window, cx| {
                cx.new(|cx| Workspace::new(start, window, cx))
            });
            if let Err(error) = opened {
                eprintln!("aturfile: could not open a window: {error}");
                std::process::exit(1);
            }
            cx.on_window_closed(|cx, _| {
                if cx.windows().is_empty() {
                    cx.quit();
                }
            })
            .detach();
            cx.activate(true);
        });
}
