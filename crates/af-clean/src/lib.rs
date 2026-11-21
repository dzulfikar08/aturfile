//! af-clean: the disk cleanup engine behind aturfile.
//!
//! One scan produces a [`model::Node`] tree; every feature reads that tree:
//! visualizations, Age Map, duplicates, System Data ("Other") report, and a
//! staged cleanup queue that never deletes anything without an explicit,
//! reviewed [`cleanup::apply`] call.

pub mod analysis;
pub mod apps;
pub mod cleanup;
pub mod dupes;
pub mod model;
pub mod other;
pub mod paths;
pub mod scan;
pub mod snapshot;
pub mod sysdata;

pub use model::{Kind, Node, ScanResult};
pub use scan::{scan, Progress, ScanOptions};

#[derive(Debug, thiserror::Error)]
pub enum DbError {
    #[error("{0}")]
    Io(#[from] std::io::Error),
    #[error("{0}")]
    Json(#[from] serde_json::Error),
    #[error("{0}")]
    Refused(String),
    #[error(transparent)]
    Plist(#[from] plist::Error),
}

pub type DbResult<T> = Result<T, DbError>;
