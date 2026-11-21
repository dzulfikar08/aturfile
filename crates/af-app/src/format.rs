//! Dates and sizes as the views show them.

use chrono::{Local, TimeZone};

/// "Just now", "5 min ago", "Yesterday", then the date.
pub fn relative_time(secs: i64) -> String {
    let now = chrono::Utc::now().timestamp();
    let age = now - secs;
    if age < 0 {
        return short_date(secs);
    }
    match age {
        0..60 => "Just now".into(),
        60..3600 => format!("{} min ago", age / 60),
        3600..86_400 => format!("{} h ago", age / 3600),
        86_400..172_800 => "Yesterday".into(),
        _ => short_date(secs),
    }
}

/// `2026-11-18` in local time.
pub fn short_date(secs: i64) -> String {
    match Local.timestamp_opt(secs, 0).single() {
        Some(t) => t.format("%Y-%m-%d").to_string(),
        None => String::new(),
    }
}

/// `18 Nov 2026 at 14:05` in local time.
pub fn long_date(secs: i64) -> String {
    match Local.timestamp_opt(secs, 0).single() {
        Some(t) => t.format("%-d %b %Y at %H:%M").to_string(),
        None => String::new(),
    }
}

/// "1 item", "24 items".
pub fn items(n: usize) -> String {
    if n == 1 {
        "1 item".into()
    } else {
        format!("{n} items")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn counts_and_dates() {
        assert_eq!(items(1), "1 item");
        assert_eq!(items(0), "0 items");
        assert_eq!(relative_time(chrono::Utc::now().timestamp()), "Just now");
        assert_eq!(short_date(1_795_000_000).len(), 10);
        assert!(long_date(1_795_000_000).contains("2026"));
    }
}
