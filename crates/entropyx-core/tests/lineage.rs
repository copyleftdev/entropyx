//! RFC-004 invariants at the interning layer.
//!
//! The production lineage resolver (union-find over rename chains) lives in
//! `entropyx-git` and is not yet scaffolded. These tests exercise only the
//! contract `VertexTable` provides: when two historical paths resolve to the
//! same lineage key, they intern to the same FileId. That is the mechanism
//! by which renames do not fragment metrics.

use entropyx_core::VertexTable;

#[test]
fn same_lineage_key_returns_same_file_id() {
    let mut v = VertexTable::new();
    let key = "lineage:7f3a"; // resolver output, not a path
    let id_pre_rename = v.intern_file(key);
    let id_post_rename = v.intern_file(key);
    assert_eq!(
        id_pre_rename, id_post_rename,
        "RFC-004: post-rename lookup must re-use the pre-rename FileId"
    );
}

#[test]
fn distinct_lineage_keys_are_distinct_ids() {
    let mut v = VertexTable::new();
    let a = v.intern_file("lineage:aaaa");
    let b = v.intern_file("lineage:bbbb");
    assert_ne!(a, b);
}

#[test]
fn author_interning_is_stable_and_case_sensitive() {
    // Case normalization is the *caller's* responsibility (identity layer in
    // entropyx-github per RFC-010); VertexTable treats keys as opaque bytes.
    let mut v = VertexTable::new();
    let a1 = v.intern_author("alice@example.com");
    let a2 = v.intern_author("alice@example.com");
    let a3 = v.intern_author("Alice@Example.com");
    assert_eq!(a1, a2);
    assert_ne!(
        a1, a3,
        "VertexTable must not normalize; identity layer does"
    );
}

#[test]
fn round_trip_preserves_interned_strings() {
    let mut v = VertexTable::new();
    let f = v.intern_file("lineage:xyz");
    let c = v.intern_commit("deadbeef".repeat(5).as_str());
    let a = v.intern_author("alice@example.com");

    let json = serde_json::to_string(&v).expect("serialize");
    let mut back: VertexTable = serde_json::from_str(&json).expect("deserialize");
    back.rehydrate();

    assert_eq!(back.file(f), Some("lineage:xyz"));
    assert_eq!(back.commit(c).map(str::len), Some(40));
    assert_eq!(back.author(a), Some("alice@example.com"));

    // After rehydrate, re-interning known keys must return the original ids.
    assert_eq!(back.intern_file("lineage:xyz"), f);
    assert_eq!(back.intern_author("alice@example.com"), a);
}
