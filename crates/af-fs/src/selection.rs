//! Multi-selection over a list of `len` rows, Finder style: click selects
//! one, Cmd-click toggles, Shift-click and Shift-arrows extend a range from
//! the anchor, Cmd-A selects everything.

use std::collections::BTreeSet;

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Selection {
    /// The row keyboard moves start from (and the one previewed).
    cursor: Option<usize>,
    /// Where Shift ranges start.
    anchor: usize,
    rows: BTreeSet<usize>,
}

impl Selection {
    /// Nothing selected.
    pub fn new() -> Self {
        Self::default()
    }

    /// Just `ix`.
    pub fn single(ix: usize) -> Self {
        let mut s = Self::new();
        s.select_only(ix);
        s
    }

    pub fn clear(&mut self) {
        self.rows.clear();
        self.cursor = None;
        self.anchor = 0;
    }

    pub fn cursor(&self) -> Option<usize> {
        self.cursor
    }

    pub fn count(&self) -> usize {
        self.rows.len()
    }

    pub fn is_empty(&self) -> bool {
        self.rows.is_empty()
    }

    pub fn contains(&self, ix: usize) -> bool {
        self.rows.contains(&ix)
    }

    /// Selected rows, ascending.
    pub fn rows(&self) -> impl Iterator<Item = usize> + '_ {
        self.rows.iter().copied()
    }

    /// A plain click or arrow key: only `ix`.
    pub fn select_only(&mut self, ix: usize) {
        self.rows.clear();
        self.rows.insert(ix);
        self.cursor = Some(ix);
        self.anchor = ix;
    }

    /// Cmd-click: add or remove `ix`. An added row becomes the anchor; after
    /// a removal the nearest row still selected does.
    pub fn toggle(&mut self, ix: usize) {
        let at = if self.rows.remove(&ix) {
            self.rows
                .iter()
                .copied()
                .min_by_key(|r| r.abs_diff(ix))
                .unwrap_or(ix)
        } else {
            self.rows.insert(ix);
            ix
        };
        self.cursor = Some(at);
        self.anchor = at;
    }

    /// Shift-click / Shift-arrow: the range from the anchor to `ix`.
    pub fn extend_to(&mut self, ix: usize) {
        if self.cursor.is_none() {
            self.select_only(ix);
            return;
        }
        let (lo, hi) = (self.anchor.min(ix), self.anchor.max(ix));
        self.rows = (lo..=hi).collect();
        self.cursor = Some(ix);
    }

    /// Cmd-A.
    pub fn select_all(&mut self, len: usize) {
        if len == 0 {
            self.clear();
            return;
        }
        self.rows = (0..len).collect();
        let cursor = self.cursor.unwrap_or(0).min(len - 1);
        self.cursor = Some(cursor);
    }

    /// Arrow keys: move the cursor by `delta` rows (clamped to the list),
    /// extending the range with Shift. With nothing selected, starts at the
    /// first row (or the last, moving up).
    pub fn move_by(&mut self, delta: isize, len: usize, extend: bool) {
        if len == 0 {
            return;
        }
        let target = match self.cursor {
            None if delta < 0 => len - 1,
            None => 0,
            Some(c) => (c as isize + delta).clamp(0, len as isize - 1) as usize,
        };
        if extend {
            self.extend_to(target);
        } else {
            self.select_only(target);
        }
    }

    /// Jump to `ix` (Home/End, Page keys), extending with Shift.
    pub fn move_to(&mut self, ix: usize, len: usize, extend: bool) {
        if len == 0 {
            return;
        }
        let ix = ix.min(len - 1);
        if extend {
            self.extend_to(ix);
        } else {
            self.select_only(ix);
        }
    }

    /// Drop rows past `len` (the list got shorter).
    pub fn clamp(&mut self, len: usize) {
        self.rows.retain(|&r| r < len);
        if len == 0 {
            self.clear();
            return;
        }
        if let Some(c) = self.cursor
            && c >= len
        {
            self.cursor = Some(len - 1);
        }
        self.anchor = self.anchor.min(len - 1);
    }

    /// Rebuild from rows found again after a re-sort or re-filter.
    pub fn from_rows(rows: impl IntoIterator<Item = usize>, cursor: Option<usize>) -> Self {
        let rows: BTreeSet<usize> = rows.into_iter().collect();
        let cursor = cursor.or_else(|| rows.iter().next().copied());
        Self {
            anchor: cursor.unwrap_or(0),
            cursor,
            rows,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rows(s: &Selection) -> Vec<usize> {
        s.rows().collect()
    }

    #[test]
    fn click_toggle_and_range() {
        let mut s = Selection::single(2);
        s.toggle(5);
        assert_eq!(rows(&s), [2, 5]);
        s.toggle(2);
        assert_eq!(rows(&s), [5]);
        // Shift-click ranges from the last toggled row.
        s.extend_to(7);
        assert_eq!(rows(&s), [5, 6, 7]);
        s.extend_to(3);
        assert_eq!(rows(&s), [3, 4, 5]);
        assert_eq!(s.cursor(), Some(3));
        s.select_only(1);
        assert_eq!(rows(&s), [1]);
    }

    #[test]
    fn arrows_extend_and_clamp() {
        let mut s = Selection::new();
        s.move_by(1, 10, false);
        assert_eq!(rows(&s), [0]);
        s.move_by(1, 10, true);
        s.move_by(1, 10, true);
        assert_eq!(rows(&s), [0, 1, 2]);
        s.move_by(-1, 10, true);
        assert_eq!(rows(&s), [0, 1]);
        s.move_by(100, 10, false);
        assert_eq!(rows(&s), [9]);
        let mut up = Selection::new();
        up.move_by(-1, 4, false);
        assert_eq!(up.cursor(), Some(3));
    }

    #[test]
    fn select_all_and_shrink() {
        let mut s = Selection::single(3);
        s.select_all(5);
        assert_eq!(s.count(), 5);
        assert_eq!(s.cursor(), Some(3));
        s.clamp(2);
        assert_eq!(rows(&s), [0, 1]);
        assert_eq!(s.cursor(), Some(1));
        s.clamp(0);
        assert!(s.is_empty());
        assert_eq!(s.cursor(), None);
        let mut e = Selection::new();
        e.select_all(0);
        assert!(e.is_empty());
    }

    #[test]
    fn rebuilt_from_rows() {
        let s = Selection::from_rows([4, 1], None);
        assert_eq!(rows(&s), [1, 4]);
        assert_eq!(s.cursor(), Some(1));
    }
}
