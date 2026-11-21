# aturfile

A fast file manager for macOS with disk cleanup built in. Rust and GPUI.

The core is open source (GPL-3.0-or-later). A paid Pro build is sold on the
Mac App Store and as a direct download (one-time licence, one year of updates).

## Layout

```
crates/af-fs     folder listing: parallel stat, natural sort, type-to-filter
crates/af-mac    macOS glue: real home, Finder Trash, sandbox folder grants
crates/af-clean  cleanup engine: parallel scanner, System Data report,
                 staged queue with protected-path guards, duplicates
crates/af-app    the GPUI app (binary `aturfile`)
```

GPUI comes from the VoltrusID fork (see `Cargo.toml`), the same one Voltrus
Tools ships to the App Store.

## Builds

GitHub Actions builds everything (`.github/workflows/ci.yml`): the engines
are linted and tested on Linux, and the app is linted, tested, built,
bundled and launched on macOS. Each run uploads `aturfile-macos` (a plain
bundle plus one signed ad hoc with the App Store sandbox entitlements) and
`smoke-screenshots`.

```
aturfile [--cleanup] [folder]
```

## Keys

| Key | Does |
| --- | --- |
| Up / Down, Home / End, Page Up / Down | Move |
| Enter, Cmd-Down | Open |
| Backspace (empty filter), Cmd-Up | Enclosing folder |
| Cmd-[ / Cmd-] | Back / Forward |
| Any letter | Filter this folder |
| Escape | Clear the filter |
| Space | Quick Look |
| Cmd-Backspace | Move to Trash |
| Cmd-Shift-. | Show hidden files |
| Cmd-1 / Cmd-2 | Files / Clean Up |
| Cmd-S or Enter (Clean Up) | Stage or unstage the selected row |
| Cmd-Shift-Backspace (Clean Up) | Move the queue to Trash |

On Linux, Cmd is Ctrl.

## Contributing

Contributions need a signed contributor licence agreement ([CLA.md](CLA.md)),
so the same code can be sold on the App Store. The CLA bot asks on your first
pull request.
