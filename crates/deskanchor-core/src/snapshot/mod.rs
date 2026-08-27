//! Versioned snapshots, local storage, and pure diff/matching logic.

mod diff;
mod model;
mod storage;

pub use diff::{
    AmbiguousIcon, IconPosition, MovedIcon, SnapshotDiff, SnapshotDiffSummary, diff_desktop,
};
pub use model::{CURRENT_SCHEMA_VERSION, Snapshot, SnapshotError};
pub use storage::{SnapshotStore, StoredSnapshot};
