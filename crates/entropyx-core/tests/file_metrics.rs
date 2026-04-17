//! Per-file physics primitives: change_counts, unit_normalize.
//!
//! Weighted-degree coupling was historically computed here via
//! `coupling_degree`. That function has been retired — the scan
//! pipeline now uses `entropyx_graph::CoChangeGraph::weighted_degree`,
//! which returns the same value but is exposed alongside richer graph
//! metrics (betweenness, etc.).

use entropyx_core::metric::{change_counts, unit_normalize};
use std::collections::BTreeMap;

#[test]
fn change_counts_tallies_per_path() {
    let commits = vec![vec!["a.rs", "b.rs"], vec!["a.rs"], vec!["b.rs", "c.rs"]];
    let counts = change_counts(&commits);
    assert_eq!(counts.get("a.rs"), Some(&2));
    assert_eq!(counts.get("b.rs"), Some(&2));
    assert_eq!(counts.get("c.rs"), Some(&1));
    assert_eq!(counts.len(), 3);
}

#[test]
fn change_counts_empty_is_empty() {
    let commits: Vec<Vec<&str>> = vec![];
    assert!(change_counts(&commits).is_empty());
}

#[test]
fn unit_normalize_scales_to_one() {
    let mut raw = BTreeMap::new();
    raw.insert("x".to_string(), 4u64);
    raw.insert("y".to_string(), 2u64);
    raw.insert("z".to_string(), 0u64);
    let n = unit_normalize(&raw);
    assert!((n["x"] - 1.0).abs() < 1e-12);
    assert!((n["y"] - 0.5).abs() < 1e-12);
    assert_eq!(n["z"], 0.0);
}

#[test]
fn unit_normalize_all_zero_stays_zero() {
    let mut raw = BTreeMap::new();
    raw.insert("x".to_string(), 0u64);
    raw.insert("y".to_string(), 0u64);
    let n = unit_normalize(&raw);
    assert_eq!(n["x"], 0.0);
    assert_eq!(n["y"], 0.0);
}

#[test]
fn unit_normalize_empty_is_empty() {
    let raw: BTreeMap<String, u64> = BTreeMap::new();
    assert!(unit_normalize(&raw).is_empty());
}

#[test]
fn change_counts_is_order_invariant() {
    // RFC-001: bitwise-stable regardless of how callers build the input
    // (BTreeMap sorting + deterministic iteration protects us).
    let c1 = vec![vec!["a.rs", "b.rs"], vec!["b.rs", "c.rs"]];
    let c2 = vec![vec!["b.rs", "a.rs"], vec!["c.rs", "b.rs"]];
    assert_eq!(change_counts(&c1), change_counts(&c2));
}
