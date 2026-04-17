//! RFC-008 ownership-split detection: "file went from one owner to many".

use entropyx_core::metric::detect_ownership_split;

#[test]
fn clear_split_after_established_ownership() {
    // alice owns 3 leading commits, then bob joins at t=400.
    let pairs = [
        (100, "alice"),
        (200, "alice"),
        (300, "alice"),
        (400, "bob"),
    ];
    let (at, authors) = detect_ownership_split(&pairs).expect("split detected");
    assert_eq!(at, 400);
    assert_eq!(authors, vec!["alice", "bob"]);
}

#[test]
fn minimum_two_consecutive_by_owner() {
    // Only ONE commit by alice before bob joins → no established ownership.
    let pairs = [(100, "alice"), (200, "bob"), (300, "bob")];
    assert_eq!(detect_ownership_split(&pairs), None);
}

#[test]
fn single_author_never_splits() {
    let pairs = [(100, "alice"), (200, "alice"), (300, "alice")];
    assert_eq!(detect_ownership_split(&pairs), None);
}

#[test]
fn fewer_than_three_touches_never_splits() {
    let pairs = [(100, "alice"), (200, "bob")];
    assert_eq!(detect_ownership_split(&pairs), None);
}

#[test]
fn alternating_authors_from_start_do_not_split() {
    // alice-bob-alice-bob: first author only has 1 leading commit.
    let pairs = [
        (100, "alice"),
        (200, "bob"),
        (300, "alice"),
        (400, "bob"),
    ];
    assert_eq!(detect_ownership_split(&pairs), None);
}

#[test]
fn split_collects_all_distinct_authors() {
    // Once the split fires, all authors who ever touched the file should
    // appear in the output, deduplicated and sorted.
    let pairs = [
        (100, "alice"),
        (200, "alice"),
        (300, "bob"),
        (400, "carol"),
        (500, "alice"),
    ];
    let (at, authors) = detect_ownership_split(&pairs).expect("split");
    assert_eq!(at, 300);
    assert_eq!(authors, vec!["alice", "bob", "carol"]);
}
