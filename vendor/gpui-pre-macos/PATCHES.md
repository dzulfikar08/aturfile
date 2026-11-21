# gpui-pre-macos 0.3.12, patched for the Mac App Store

Copied from crates.io (`gpui-pre-macos` 0.3.12, a snapshot of Zed's `gpui_macos`,
Apache-2.0, see `LICENSE-APACHE`). App Review rejects apps that call private
AppKit API, so `src/window.rs` swaps every private call for a public one. Each
change is marked with an `aturfile:` comment.

| Upstream (private) | Here (public) | What changes for users |
|---|---|---|
| Override of `-[NSView _opaqueRectForWindowMoveWhenInTitlebar]` | Override of `-[NSView mouseDownCanMoveWindow]` | Nothing expected: clicks in an app-drawn titlebar still don't move the window. |
| Override of `-[NSKeyedUnarchiver _windowRestorationOptions]` and the private `NSWindowRestorationOptions` class | Removed | On macOS 15+, restored windows may open on the current Space instead of their old one. |
| `-[NSWindow _zoomFill:]` for the "Fill" titlebar double-click action | `-[NSWindow zoom:]` | The Fill action ignores "Tiled windows have margins". |
| `+[NSCursor _windowResizeNorthWestSouthEastCursor]` and `…NorthEastSouthWest…` | `+[NSCursor frameResizeCursorFromPosition:inDirections:]` (macOS 15+), crosshair before | Same diagonal cursors on macOS 15+. |

To move to a newer gpui-pre: copy the new release here, then re-apply these
changes and check with
`grep -rnE 'sel!\(_|msg_send!\[[^]]*, _[a-zA-Z]' src`, which should print nothing.
