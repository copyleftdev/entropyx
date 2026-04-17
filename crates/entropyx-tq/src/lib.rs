//! tq1 — token-efficient summary protocol for entropyx (RFC-009).
//!
//! Two-layer protocol: the `Summary` is dense and dictionary-encoded;
//! evidence is addressable by `Handle` and fetched on demand. AI
//! consumers pay tokens only for the zones they choose to investigate.
//!
//! Lives in its own crate so the protocol envelope can evolve without
//! cycling `entropyx-core` (which holds the underlying primitive
//! types: `FileId`, `AuthorId`, `Timestamp`, `SignalClass`,
//! `Handle`, `VertexTable`).

use entropyx_core::handle::Handle;
use entropyx_core::id::{AuthorId, FileId, Timestamp};
use entropyx_core::metric::SignalClass;
use entropyx_core::vertex::VertexTable;
use entropyx_core::{CONTRACT_VERSION, SCHEMA};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Serialize, Deserialize)]
pub struct Summary {
    pub schema: Schema,
    pub dict: Dict,
    pub files: Vec<FileRow>,
    pub events: Vec<Event>,
    /// Handles are keyed by `Handle::key()` so the summary is self-indexed.
    pub handles: BTreeMap<String, Handle>,
    /// External-source enrichments keyed by commit SHA. Populated by
    /// `scan --github` (and future enrichers). Empty by default.
    #[serde(default)]
    pub enrichments: Enrichments,
}

/// Sidecar for external metadata. Keyed on git commit SHA so callers
/// can join enrichments to any event that carries a sha field.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Enrichments {
    #[serde(default)]
    pub pull_requests: BTreeMap<String, PullRequestRef>,
}

/// External-source metadata attached to commits in the tq1 Summary.
/// Neutral type (no network deps) so the `Summary` sidecar can be read
/// without any enricher crate as a dependency.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestRef {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub merged_at: Option<String>,
    pub author: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Schema {
    pub name: String,
    pub version: String,
}

impl Default for Schema {
    fn default() -> Self {
        Self {
            name: SCHEMA.to_string(),
            version: CONTRACT_VERSION.to_string(),
        }
    }
}

/// String dictionaries extracted from the `VertexTable` at summary time.
/// `metrics` pins the column order of `FileRow::values` for the decoder.
#[derive(Debug, Serialize, Deserialize)]
pub struct Dict {
    pub files: Vec<String>,
    pub authors: Vec<String>,
    pub metrics: Vec<String>,
}

impl Dict {
    /// Column order is load-bearing: `FileRow::values[i]` aligns to
    /// `METRIC_COLUMNS[i]`. Changes to this array are breaking and must
    /// bump `CONTRACT_VERSION`.
    pub const METRIC_COLUMNS: [&'static str; 8] = [
        "change_density",
        "author_entropy",
        "temporal_volatility",
        "coupling_stress",
        "blame_youth",
        "semantic_drift",
        "test_cooevolution",
        "composite",
    ];

    pub fn from_vertex(v: &VertexTable) -> Self {
        Self {
            files: v.files.clone(),
            authors: v.authors.clone(),
            metrics: Self::METRIC_COLUMNS.iter().map(|s| s.to_string()).collect(),
        }
    }
}

/// Dense per-file row. Column order matches `Dict::METRIC_COLUMNS`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct FileRow {
    pub file: FileId,
    pub values: [f64; 8],
    pub lineage_confidence: f64,
    pub signal_class: Option<SignalClass>,
}

/// Discrete event on a file lineage, ordered chronologically per file.
///
/// Every variant carries a `sha` field — the full 40-char commit hash
/// of the commit that produced the event. Consumers join it against
/// `Summary.enrichments.pull_requests` to surface PR / review context
/// alongside the event.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Hotspot {
        file: FileId,
        at: Timestamp,
        #[serde(default)]
        sha: String,
        reason: String,
    },
    OwnershipSplit {
        file: FileId,
        at: Timestamp,
        #[serde(default)]
        sha: String,
        authors: Vec<AuthorId>,
    },
    ApiDrift {
        file: FileId,
        at: Timestamp,
        #[serde(default)]
        sha: String,
        pub_items_changed: u32,
    },
    Rename {
        file: FileId,
        at: Timestamp,
        #[serde(default)]
        sha: String,
        from: String,
        to: String,
    },
    IncidentAftershock {
        file: FileId,
        at: Timestamp,
        #[serde(default)]
        sha: String,
        window_days: u32,
    },
}

pub mod schema;
