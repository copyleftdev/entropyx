//! External-source metadata attached to commits or files in the tq1
//! Summary. The types are intentionally neutral (no network types) so
//! the `entropyx-core` layering discipline is preserved — enrichers
//! (entropyx-github, future ones) populate these, and the `Summary`
//! sidecar can be read without any enricher crate as a dependency.

use serde::{Deserialize, Serialize};

/// A merged or open pull request that references a commit. Populated
/// by `entropyx-github` when scan/explain are invoked with a GitHub
/// enrichment flag; absent otherwise.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct PullRequestRef {
    pub number: u64,
    pub title: String,
    pub state: String,
    pub merged: bool,
    pub merged_at: Option<String>,
    pub author: Option<String>,
}
