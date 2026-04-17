//! RFC-009 invariants: Summary and Describe serialize losslessly through JSON
//! and preserve the protocol-critical fields.

use entropyx_core::{Describe, Handle, Timestamp, VertexTable};
use entropyx_tq::{Dict, Event, FileRow, Schema, Summary};
use std::collections::BTreeMap;

#[test]
fn summary_round_trips_through_json() {
    let mut v = VertexTable::new();
    let fid = v.intern_file("lineage:abc");
    let _aid = v.intern_author("alice@example.com");
    let _bid = v.intern_author("bob@example.com");

    let mut handles = BTreeMap::new();
    let h = Handle::file(fid, "abcdef012345ffff");
    handles.insert(h.key(), h.clone());

    let s = Summary {
        schema: Schema::default(),
        dict: Dict::from_vertex(&v),
        files: vec![FileRow {
            file: fid,
            values: [0.73, 0.41, 0.88, 0.62, 0.19, 0.95, 0.33, 0.61],
            lineage_confidence: 0.95,
            signal_class: None,
        }],
        events: vec![Event::Hotspot {
            file: fid,
            at: Timestamp(1_700_000_000),
            sha: "abcdef0123456789abcdef0123456789abcdef01".into(),
            reason: "burst_refactor".into(),
        }],
        handles,
        enrichments: Default::default(),
    };

    let json = serde_json::to_string(&s).expect("serialize");
    let back: Summary = serde_json::from_str(&json).expect("deserialize");

    assert_eq!(back.dict.files, s.dict.files);
    assert_eq!(back.dict.authors, s.dict.authors);
    assert_eq!(back.dict.metrics, s.dict.metrics);
    assert_eq!(back.files[0].values, s.files[0].values);
    assert_eq!(back.files[0].file, fid);
    assert_eq!(back.handles.len(), 1);
    assert!(back.handles.contains_key(&h.key()));
}

#[test]
fn metric_columns_match_filerow_width() {
    assert_eq!(
        Dict::METRIC_COLUMNS.len(),
        8,
        "FileRow::values is a fixed-size [f64; 8] aligned to METRIC_COLUMNS"
    );
    assert_eq!(Dict::METRIC_COLUMNS[7], "composite");
}

#[test]
fn schema_default_reports_contract_version() {
    let s = Schema::default();
    assert_eq!(s.name, entropyx_core::SCHEMA);
    assert_eq!(s.version, entropyx_core::CONTRACT_VERSION);
}

#[test]
fn describe_serializes_and_declares_invariants() {
    let d = Describe::current();
    let json = serde_json::to_string(&d).expect("describe serializes");
    // Must be self-describing enough for an AI consumer to bootstrap.
    assert!(json.contains("\"name\":\"entropyx\""));
    assert!(json.contains("\"contract_version\""));
    assert!(json.contains("\"capabilities\""));
    assert!(json.contains("deterministic"));
    assert!(json.contains("local-first"));
    assert!(json.contains("lineage-keyed"));
}
