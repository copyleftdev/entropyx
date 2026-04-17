//! Commit metadata surface. All values are owned so higher layers can
//! intern them into `entropyx_core::VertexTable` (RFC-004) without
//! lifetime ties to gix internals, and so the types round-trip cleanly
//! through serde without the `&'static str` / `Deserialize` conflict
//! documented on `Describe` and `Summary`.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct CommitMeta {
    /// 40-char lowercase hex object id.
    pub sha: String,
    /// Parent commit SHAs in the order gix yields them (first parent first
    /// on non-merge linearizations — load-bearing for RFC-003 walks).
    pub parents: Vec<String>,
    /// Root tree SHA.
    pub tree: String,
    pub author: Signature,
    pub committer: Signature,
    /// First line of the commit message, UTF-8 lossy.
    pub subject: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Signature {
    pub name: String,
    pub email: String,
    /// Unix epoch seconds, UTC. Timezone offset is intentionally dropped:
    /// the physics layer operates in UTC (RFC-001 determinism) and per-
    /// author tz would break bitwise reproducibility across machines.
    pub time: i64,
}
