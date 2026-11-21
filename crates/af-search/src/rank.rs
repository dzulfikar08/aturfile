//! Fuzzy ranking. A query matches when its characters appear in order in
//! the name (or, for queries with `/` or several words, in the path).
//! Exact names beat prefixes, prefixes beat contiguous runs, contiguous runs
//! beat scattered letters, and letters at word or path boundaries count more.

use crate::index::Item;

/// A ranked result: the item's position in the index and its score.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Hit {
    pub ix: usize,
    pub score: i32,
}

fn is_boundary(prev: Option<char>) -> bool {
    matches!(prev, None | Some(' ' | '-' | '_' | '.' | '/' | '(' | '['))
}

/// Score `needle` (lowercase) against `hay`; `None` when it doesn't match.
fn fuzzy(needle: &str, hay: &str) -> Option<i32> {
    if needle.is_empty() {
        return Some(0);
    }
    let lower = hay.to_lowercase();
    if lower == needle {
        return Some(1000);
    }
    if lower.starts_with(needle) {
        return Some(800 - (lower.len() - needle.len()).min(100) as i32);
    }
    if let Some(at) = lower.find(needle) {
        let prev = lower[..at].chars().next_back();
        let bonus = if is_boundary(prev) { 120 } else { 0 };
        return Some(600 + bonus - (at.min(100) as i32) - (lower.len().min(200) as i32) / 4);
    }
    // Scattered: walk the haystack, rewarding boundary hits and runs.
    let mut score = 300;
    let mut prev: Option<char> = None;
    let mut run = 0;
    let mut chars = needle.chars().peekable();
    let mut gaps = 0;
    for c in lower.chars() {
        match chars.peek() {
            None => break,
            Some(&n) if n == c => {
                chars.next();
                if is_boundary(prev) {
                    score += 25;
                }
                run += 1;
                score += run * 4;
            }
            Some(_) => {
                run = 0;
                gaps += 1;
            }
        }
        prev = Some(c);
    }
    if chars.peek().is_some() {
        return None;
    }
    Some(score - gaps.min(150))
}

/// Score `query` against an item. Words must all match: in the name for a
/// single word, otherwise anywhere in the path (each word scored apart).
pub fn score(query: &str, item: &Item) -> Option<i32> {
    let q = query.trim().to_lowercase();
    if q.is_empty() {
        return None;
    }
    let words: Vec<&str> = q.split_whitespace().collect();
    let base = if words.len() == 1 && !q.contains('/') {
        // Name first; the path is a weaker fallback.
        match fuzzy(&q, item.name()) {
            Some(s) => s,
            None => fuzzy(&q, &item.path).map(|s| s / 3)?,
        }
    } else {
        let mut total = 0;
        for w in &words {
            let s = match fuzzy(w, item.name()) {
                Some(s) => s,
                None => fuzzy(w, &item.path).map(|s| s / 3)?,
            };
            total += s;
        }
        total / words.len() as i32
    };
    // Shallower paths and folders first among equals.
    let depth = item.path.matches('/').count() as i32;
    Some(base * 4 - depth + if item.is_dir { 2 } else { 0 })
}

/// The best `limit` matches for `query` among `items`, best first.
pub fn search(query: &str, items: &[Item], limit: usize) -> Vec<Hit> {
    let mut hits: Vec<Hit> = items
        .iter()
        .enumerate()
        .filter_map(|(ix, item)| score(query, item).map(|score| Hit { ix, score }))
        .collect();
    let by_rank = |a: &Hit, b: &Hit| {
        b.score
            .cmp(&a.score)
            .then_with(|| items[a.ix].path.len().cmp(&items[b.ix].path.len()))
    };
    if hits.len() > limit {
        hits.select_nth_unstable_by(limit, by_rank);
        hits.truncate(limit);
    }
    hits.sort_by(by_rank);
    hits
}

#[cfg(test)]
mod tests {
    use super::*;

    fn items(paths: &[&str]) -> Vec<Item> {
        paths
            .iter()
            .map(|p| Item::new(p.to_string(), p.ends_with('/')))
            .collect()
    }

    fn names(query: &str, items: &[Item]) -> Vec<String> {
        search(query, items, 100)
            .iter()
            .map(|h| items[h.ix].name().to_owned())
            .collect()
    }

    #[test]
    fn exact_then_prefix_then_contiguous_then_scattered() {
        let v = items(&[
            "/u/a/report-old-final.txt",
            "/u/a/my report.pdf",
            "/u/a/report",
            "/u/a/reporting.md",
            "/u/a/rxexpxort.txt",
        ]);
        assert_eq!(
            names("report", &v),
            [
                "report",
                "reporting.md",
                "report-old-final.txt",
                "my report.pdf",
                "rxexpxort.txt"
            ]
        );
    }

    #[test]
    fn boundaries_count() {
        let v = items(&["/u/notes/xcargotoml", "/u/notes/Cargo.toml"]);
        assert_eq!(names("ctoml", &v)[0], "Cargo.toml");
        let v = items(&["/u/a/sparkle.rs", "/u/a/park.rs"]);
        assert_eq!(names("park", &v)[0], "park.rs");
    }

    #[test]
    fn words_and_paths() {
        let v = items(&[
            "/u/Projects/aturfile/src/main.rs",
            "/u/Projects/kantor/src/main.rs",
            "/u/Downloads/main.rs.bak",
        ]);
        let hits = search("aturfile main", &v, 10);
        assert_eq!(hits.len(), 1);
        assert_eq!(v[hits[0].ix].path, "/u/Projects/aturfile/src/main.rs");
        assert_eq!(search("kantor/src", &v, 10).len(), 1);
        assert!(search("zzz", &v, 10).is_empty());
        assert!(search("   ", &v, 10).is_empty());
    }

    #[test]
    fn keeps_the_top_n_in_order() {
        let paths: Vec<String> = (0..500).map(|i| format!("/u/file{i}.txt")).collect();
        let refs: Vec<&str> = paths.iter().map(String::as_str).collect();
        let v = items(&refs);
        let hits = search("file1", &v, 100);
        assert_eq!(hits.len(), 100);
        assert!(hits.windows(2).all(|w| w[0].score >= w[1].score));
        assert_eq!(v[hits[0].ix].name(), "file1.txt");
    }
}
