//! Every action and key binding, in one place. Bindings are written for
//! macOS (`cmd-`); `af_ui::keys::primary` makes them Ctrl elsewhere.

use af_ui::keys::primary;
use gpui_kit::prelude::*;
use gpui_kit::*;

pub const WORKSPACE: &str = "Workspace";
pub const PALETTE: &str = "Palette";
/// The palette's text field: arrows move the highlight, not the caret.
const PALETTE_INPUT: &str = "Palette > Input";
/// The filter field: arrows move the file selection.
const FILTER_INPUT: &str = "Filter > Input";

actions!(
    aturfile,
    [
        Quit,
        // Shell
        NewTab,
        CloseTab,
        NextTab,
        PrevTab,
        TogglePalette,
        OpenSettings,
        Cancel,
        ToggleSidebar,
        TogglePreview,
        GetInfo,
        ShowList,
        ShowColumns,
        ShowIcons,
        ShowCleanup,
        FocusFilter,
        EditLocation,
        PaletteUp,
        PaletteDown,
        // Moving and selecting
        SelectNext,
        SelectPrev,
        SelectLeft,
        SelectRight,
        ExtendNext,
        ExtendPrev,
        ExtendLeft,
        ExtendRight,
        SelectFirst,
        SelectLast,
        PageDown,
        PageUp,
        SelectAll,
        // Going places
        Open,
        Parent,
        Back,
        Forward,
        GoHome,
        Reload,
        // Items
        ToggleHidden,
        MoveToTrash,
        Reveal,
        CopyPath,
        QuickLook,
        NewFolder,
        Rename,
        Duplicate,
        CopyItems,
        CutItems,
        PasteItems,
        MoveItemsHere,
        Undo,
        Redo,
        // Clean Up
        StageSelected,
        EmptyQueue,
    ]
);

fn ws(keys: &'static str, action: impl Action) -> KeyBinding {
    KeyBinding::new(primary(keys), action, Some(WORKSPACE))
}

pub fn bind_keys(cx: &mut App) {
    cx.bind_keys([
        KeyBinding::new(primary("cmd-q"), Quit, None),
        // Shell
        ws("cmd-t", NewTab),
        ws("cmd-w", CloseTab),
        KeyBinding::new("ctrl-tab", NextTab, Some(WORKSPACE)),
        KeyBinding::new("ctrl-shift-tab", PrevTab, Some(WORKSPACE)),
        ws("cmd-shift-]", NextTab),
        ws("cmd-shift-[", PrevTab),
        ws("cmd-k", TogglePalette),
        ws("cmd-,", OpenSettings),
        ws("escape", Cancel),
        ws("cmd-alt-s", ToggleSidebar),
        ws("cmd-alt-p", TogglePreview),
        ws("cmd-i", GetInfo),
        ws("cmd-1", ShowList),
        ws("cmd-2", ShowColumns),
        ws("cmd-3", ShowIcons),
        ws("cmd-shift-k", ShowCleanup),
        ws("cmd-f", FocusFilter),
        ws("cmd-l", EditLocation),
        KeyBinding::new("up", PaletteUp, Some(PALETTE_INPUT)),
        KeyBinding::new("down", PaletteDown, Some(PALETTE_INPUT)),
        KeyBinding::new("up", SelectPrev, Some(FILTER_INPUT)),
        KeyBinding::new("down", SelectNext, Some(FILTER_INPUT)),
        // Moving and selecting
        ws("down", SelectNext),
        ws("up", SelectPrev),
        ws("left", SelectLeft),
        ws("right", SelectRight),
        ws("shift-down", ExtendNext),
        ws("shift-up", ExtendPrev),
        ws("shift-left", ExtendLeft),
        ws("shift-right", ExtendRight),
        ws("home", SelectFirst),
        ws("end", SelectLast),
        ws("alt-up", SelectFirst),
        ws("alt-down", SelectLast),
        ws("pagedown", PageDown),
        ws("pageup", PageUp),
        ws("cmd-a", SelectAll),
        // Going places
        ws("enter", Open),
        ws("cmd-down", Open),
        ws("cmd-o", Open),
        ws("cmd-up", Parent),
        ws("cmd-[", Back),
        ws("cmd-]", Forward),
        ws("cmd-shift-h", GoHome),
        ws("cmd-shift-r", Reload),
        // Items
        ws("cmd-shift-.", ToggleHidden),
        ws("cmd-backspace", MoveToTrash),
        ws("cmd-r", Reveal),
        ws("cmd-alt-c", CopyPath),
        ws("space", QuickLook),
        ws("cmd-shift-n", NewFolder),
        ws("f2", Rename),
        ws("cmd-d", Duplicate),
        ws("cmd-c", CopyItems),
        ws("cmd-x", CutItems),
        ws("cmd-v", PasteItems),
        ws("cmd-alt-v", MoveItemsHere),
        ws("cmd-z", Undo),
        ws("cmd-shift-z", Redo),
        // Clean Up
        ws("cmd-s", StageSelected),
        ws("cmd-shift-backspace", EmptyQueue),
    ]);
    if cfg!(target_os = "macos") {
        // Finder: Return renames.
        cx.bind_keys([ws("enter", Rename)]);
    }
}
