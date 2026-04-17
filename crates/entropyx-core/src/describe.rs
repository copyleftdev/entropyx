//! Self-identifying protocol root (RFC-000, RFC-009).
//!
//! `entropyx describe --format json` emits this struct. AI consumers
//! bootstrap from it: the tool tells them what it is, what it accepts,
//! what it emits, and what invariants hold. No external docs required.

use serde::Serialize;

#[derive(Clone, Debug, Serialize)]
pub struct Describe {
    pub name: &'static str,
    pub version: &'static str,
    pub contract_version: &'static str,
    pub purpose: &'static str,
    pub capabilities: Vec<&'static str>,
    pub inputs: Inputs,
    pub outputs: Outputs,
    pub cost_model: CostModel,
    pub invariants: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct Inputs {
    pub sources: Vec<&'static str>,
    pub repo: &'static str,
    pub revisions: &'static str,
}

#[derive(Clone, Debug, Serialize)]
pub struct Outputs {
    pub formats: Vec<&'static str>,
    pub schemas: Vec<&'static str>,
}

#[derive(Clone, Debug, Serialize)]
pub struct CostModel {
    pub local_scan: &'static str,
    pub remote_enrichment: &'static str,
}

impl Describe {
    pub fn current() -> Self {
        Self {
            name: "entropyx",
            version: env!("CARGO_PKG_VERSION"),
            contract_version: crate::CONTRACT_VERSION,
            purpose: "forensic instrument for the temporal, structural, and \
                 authorship dynamics of a codebase",
            capabilities: vec![
                "scan-repo",
                "compute-entropy",
                "author-attribution",
                "hotspot-detection",
                "drift-detection",
                "rename-tracking",
                "blame-snapshot",
                "release-comparison",
                "temporal-anomaly-detection",
                "token-efficient-export",
            ],
            inputs: Inputs {
                sources: vec!["local_git", "github_rest", "github_graphql"],
                repo: "path|owner/repo",
                revisions: "sha|branch|tag|range",
            },
            outputs: Outputs {
                formats: vec!["json", "jsonl", "tq1"],
                schemas: vec!["repo_summary", "file_metric", "event", "handle"],
            },
            cost_model: CostModel {
                local_scan: "high_io_low_api",
                remote_enrichment: "low_io_rate_limited",
            },
            invariants: vec![
                "deterministic: same inputs -> same outputs, bitwise",
                "local-first: no network in core computation",
                "lineage-keyed: FileId persists across renames",
                "semantic-weighted: AST delta dominates line churn",
                "handle-addressable: summary refs evidence by content hash",
            ],
        }
    }
}
