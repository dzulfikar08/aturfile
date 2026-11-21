//! File-name search for the ⌘K dialog: an index of names and paths built on
//! a background thread (bounded by time and entry count, readable while it
//! grows), and a fuzzy ranker that keeps the best matches. Names and paths
//! only, never file contents.

mod index;
mod rank;

pub use index::{Index, IndexOptions, IndexState, Item};
pub use rank::{Hit, score, search};
