//! File-level tree-diff surface. Each `FileChange` describes one path's
//! fate between two commits. Rename detection is a first-class citizen
//! (RFC-004): renames are not modeled as Deletion + Addition pairs — they
//! carry the previous path and a similarity score so the physics layer
//! can stitch lineage without losing history at path boundaries.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum ChangeKind {
    /// New file introduced at `path`.
    Added,
    /// File at `path` removed.
    Deleted,
    /// File at `path` still exists but its content (blob id) changed.
    Modified,
    /// File moved from `from` to the outer `FileChange.path`. `similarity`
    /// is the percent match gix reports (100 for exact renames, lower
    /// for rename-with-edit).
    Renamed { from: String, similarity: u32 },
    /// File at `from` was copied to outer `FileChange.path`.
    Copied { from: String, similarity: u32 },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FileChange {
    /// Path in the *new* tree. For deletions, the path in the old tree.
    pub path: String,
    pub kind: ChangeKind,
}

impl FileChange {
    /// The path this change describes on the *old* side, if any. Useful
    /// when threading a file across renames.
    pub fn previous_path(&self) -> Option<&str> {
        match &self.kind {
            ChangeKind::Renamed { from, .. } | ChangeKind::Copied { from, .. } => Some(from),
            ChangeKind::Deleted => Some(&self.path),
            _ => None,
        }
    }
}
