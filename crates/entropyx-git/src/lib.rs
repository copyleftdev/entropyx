//! Local git collector for entropyx — the truth source for the physics
//! layer. Wraps `gix` in a facade that speaks entropyx-native owned types
//! (SHAs as hex strings, intern-ready `Signature`) so higher layers can
//! feed `entropyx_core::VertexTable` without lifetime ties to gix internals
//! (RFC-004).
//!
//! Scope note (v0.1 scaffold): `open` and the `CommitMeta` / `Signature`
//! surfaces are in place. DAG walking, diff streaming, rename detection,
//! and blame belong here and will land with their RFCs.

pub mod blame;
pub mod commit;
pub mod diff;
pub mod lineage;
pub mod repo;

pub use blame::BlameLine;
pub use commit::{CommitMeta, Signature};
pub use diff::{ChangeKind, FileChange};
pub use lineage::LineageResolver;
pub use repo::Repo;

use std::path::Path;

/// Crate-local error. Kept intentionally opaque in v0.1 so the error
/// taxonomy can solidify alongside the collector surface. Higher layers
/// should not match on variants yet.
pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;

pub type Result<T> = std::result::Result<T, Error>;

/// Open a repository at `path`. Accepts either the working tree root or
/// the `.git` directory; gix does the right thing for both.
pub fn open(path: impl AsRef<Path>) -> Result<Repo> {
    Repo::open(path)
}
