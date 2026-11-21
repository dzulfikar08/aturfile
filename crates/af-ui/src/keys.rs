//! Platform shortcuts: bindings and labels are written once with `cmd-` /
//! "Cmd", and become Ctrl on Linux and Windows.
//!
//! GPUI's own `secondary-` alias can't be used: `KeyBinding::new` parses
//! without an OS and rejects it.

/// A binding written with `cmd-` for this platform: `ctrl-` off macOS.
pub fn primary(keys: &'static str) -> &'static str {
    if cfg!(target_os = "macos") || !keys.contains("cmd-") {
        keys
    } else {
        // Bindings are made once, at start-up.
        Box::leak(keys.replace("cmd-", "ctrl-").into_boxed_str())
    }
}

/// A shortcut label written for macOS ("Cmd-Shift-N", "Cmd K") as shown on
/// this platform: "⌘⇧N", "⌘K" on macOS, "Ctrl+Shift+N" elsewhere.
pub fn label(keys: &str) -> String {
    if cfg!(target_os = "macos") {
        mac_label(keys)
    } else {
        other_label(keys)
    }
}

fn mac_label(keys: &str) -> String {
    let mut out = String::new();
    for (i, part) in keys.split(" / ").enumerate() {
        if i > 0 {
            out.push_str(" / ");
        }
        for token in part.split(['-', '+', ' ']).filter(|t| !t.is_empty()) {
            out.push_str(match token {
                "Cmd" => "⌘",
                "Shift" => "⇧",
                "Alt" | "Option" => "⌥",
                "Ctrl" => "⌃",
                "Backspace" => "⌫",
                "Enter" => "↩",
                "Escape" | "Esc" => "Esc",
                "Tab" => "⇥",
                "Up" => "↑",
                "Down" => "↓",
                "Left" => "←",
                "Right" => "→",
                "Space" => "Space",
                other => other,
            });
        }
    }
    out
}

fn other_label(keys: &str) -> String {
    let mut out = String::new();
    for (i, part) in keys.split(" / ").enumerate() {
        if i > 0 {
            out.push_str(" / ");
        }
        let tokens: Vec<&str> = part
            .split(['-', '+', ' '])
            .filter(|t| !t.is_empty())
            .map(|t| match t {
                "Cmd" => "Ctrl",
                "Option" => "Alt",
                other => other,
            })
            .collect();
        out.push_str(&tokens.join("+"));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn labels() {
        assert_eq!(mac_label("Cmd-Shift-N"), "⌘⇧N");
        assert_eq!(mac_label("Cmd K"), "⌘K");
        assert_eq!(mac_label("Cmd-[ / Cmd-]"), "⌘[ / ⌘]");
        assert_eq!(other_label("Cmd-Shift-N"), "Ctrl+Shift+N");
        assert_eq!(other_label("Cmd-Alt-S"), "Ctrl+Alt+S");
    }

    #[test]
    fn bindings() {
        assert_eq!(primary("up"), "up");
        if cfg!(target_os = "macos") {
            assert_eq!(primary("cmd-t"), "cmd-t");
        } else {
            assert_eq!(primary("cmd-t"), "ctrl-t");
        }
    }
}
