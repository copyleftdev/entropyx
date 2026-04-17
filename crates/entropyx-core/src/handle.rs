//! Content-addressed retrieval handles (RFC-009).
//!
//! A handle is a stable, string-keyed pointer from the dense summary layer
//! to evidence that an AI (or human) can fetch on demand via
//! `entropyx explain <handle>`. Because handles are content-addressed
//! (blob shas, commit shas, sha-range pairs), they cache forever against
//! immutable git objects.

use crate::id::{CommitId, FileId};
use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Handle {
    /// `file:<blob-sha-prefix-12>` — points at a file's blob at a specific
    /// observation, enough to fetch the exact bytes without ambiguity.
    File { file: FileId, blob_prefix: String },
    /// `commit:<full-sha>` — full 40-char git object hash.
    Commit { commit: CommitId, sha: String },
    /// `range:<base-sha>..<head-sha>` — inclusive commit range.
    Range { base: String, head: String },
}

impl Handle {
    pub fn file(file: FileId, blob_sha: &str) -> Self {
        Self::File {
            file,
            blob_prefix: blob_sha.chars().take(12).collect(),
        }
    }

    pub fn commit(commit: CommitId, sha: &str) -> Self {
        Self::Commit {
            commit,
            sha: sha.to_owned(),
        }
    }

    pub fn range(base: &str, head: &str) -> Self {
        Self::Range {
            base: base.to_owned(),
            head: head.to_owned(),
        }
    }

    /// Canonical string form — used as a key in `Summary.handles` and as
    /// the user-facing name AI consumers pass to `entropyx explain`.
    pub fn key(&self) -> String {
        match self {
            Handle::File { blob_prefix, .. } => format!("file:{blob_prefix}"),
            Handle::Commit { sha, .. } => format!("commit:{sha}"),
            Handle::Range { base, head } => format!("range:{base}..{head}"),
        }
    }

    /// Stable 128-bit fingerprint over the canonical key — useful as a
    /// cache key in the enrichment layer (RFC-010).
    pub fn fingerprint(&self) -> String {
        let k = self.key();
        let h = blake3::hash(k.as_bytes());
        h.to_hex().as_str()[..16].to_owned()
    }
}
