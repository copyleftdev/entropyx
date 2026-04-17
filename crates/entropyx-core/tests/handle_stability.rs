//! RFC-009 invariants: handle keys and fingerprints are stable, content-
//! addressed, and unambiguous.

use entropyx_core::{CommitId, FileId, Handle};

#[test]
fn file_handle_key_is_stable_across_construction() {
    let h1 = Handle::file(FileId(42), "abcdef012345ffff9999");
    let h2 = Handle::file(FileId(42), "abcdef012345ffff9999");
    assert_eq!(h1.key(), h2.key());
    assert_eq!(h1.fingerprint(), h2.fingerprint());
}

#[test]
fn file_handle_key_truncates_prefix_to_twelve_chars() {
    let h = Handle::file(FileId(0), "abcdef0123456789abcdef");
    assert_eq!(h.key(), "file:abcdef012345");
}

#[test]
fn commit_handle_preserves_full_sha() {
    let sha: String = "deadbeef".repeat(5);
    assert_eq!(sha.len(), 40);
    let h = Handle::commit(CommitId(1), &sha);
    assert!(h.key().starts_with("commit:"));
    assert_eq!(h.key().len(), "commit:".len() + 40);
}

#[test]
fn range_handle_round_trips_through_key() {
    let h = Handle::range("aaaaaaaa", "bbbbbbbb");
    assert_eq!(h.key(), "range:aaaaaaaa..bbbbbbbb");
}

#[test]
fn distinct_handles_produce_distinct_fingerprints() {
    let f = Handle::file(FileId(1), "aaaaaaaaaaaa");
    let c = Handle::commit(CommitId(1), &"a".repeat(40));
    let r = Handle::range("a".repeat(40).as_str(), "b".repeat(40).as_str());
    assert_ne!(f.fingerprint(), c.fingerprint());
    assert_ne!(c.fingerprint(), r.fingerprint());
    assert_ne!(f.fingerprint(), r.fingerprint());
}

#[test]
fn same_content_yields_same_fingerprint_regardless_of_id() {
    // Two handles with different interned FileIds but the same blob prefix
    // must fingerprint identically because the fingerprint is content-
    // addressed via the canonical key.
    let a = Handle::file(FileId(1), "abcdef012345");
    let b = Handle::file(FileId(99), "abcdef012345");
    assert_eq!(a.fingerprint(), b.fingerprint());
}
