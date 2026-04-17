//! entropyx-core — zero-floor deterministic root of the workspace.
//!
//! Per RFC-011, this crate has no dependency on tokio, reqwest, gix, or
//! tree-sitter. It compiles cleanly on `wasm32-unknown-unknown` as a smoke
//! test for layering discipline.
//!
//! Invariants enforced here (RFC-001):
//!   - f64 reductions go through `determinism::reduce_sum`.
//!   - No wall-clock reads; all time inputs are explicit `Timestamp` params.
//!   - Interning is insertion-ordered and stable across serde round-trips.

pub mod describe;
pub mod determinism;
pub mod enrichment;
pub mod handle;
pub mod id;
pub mod metric;
pub mod summary;
pub mod vertex;

pub use describe::Describe;
pub use enrichment::PullRequestRef;
pub use handle::Handle;
pub use id::{AuthorId, CommitId, FileId, LineageConfidence, Timestamp};
pub use metric::{Metric, MetricComponents, ScoreWeights, SignalClass};
pub use summary::{Dict, Enrichments, Event, FileRow, Schema, Summary};
pub use vertex::VertexTable;

/// tq1 — dictionary-encoded, handle-driven summary schema (RFC-009).
pub const SCHEMA: &str = "tq1";

/// Semver of the protocol contract. Bumps on any breaking change to the
/// shape of `Describe`, `Summary`, or `Handle`.
pub const CONTRACT_VERSION: &str = "0.1.0";
