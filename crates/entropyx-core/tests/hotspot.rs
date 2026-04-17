//! RFC-008 hotspot detection: the recent-burst rule.

use entropyx_core::metric::detect_recent_burst;

const THRESHOLD: f64 = 0.5;

#[test]
fn sparse_input_never_bursts() {
    assert_eq!(detect_recent_burst(&[], THRESHOLD), None);
    assert_eq!(detect_recent_burst(&[100], THRESHOLD), None);
    assert_eq!(detect_recent_burst(&[100, 200], THRESHOLD), None);
}

#[test]
fn degenerate_zero_span_never_bursts() {
    // All touches collapsed to one instant. No span = no window = no signal.
    assert_eq!(detect_recent_burst(&[500, 500, 500, 500], THRESHOLD), None);
}

#[test]
fn steady_cadence_does_not_burst() {
    // Equal spacing → ~1/4 of touches land in the last quarter by
    // definition. Threshold > 0.25 rejects this.
    assert_eq!(
        detect_recent_burst(&[100, 200, 300, 400, 500, 600, 700, 800], THRESHOLD),
        None,
    );
}

#[test]
fn recent_cluster_fires() {
    // Span 0..1000, last quarter is 750..1000.
    // Of 5 touches, 3 (900, 950, 1000) fall in that window → 60% > 50%.
    let at = detect_recent_burst(&[0, 100, 900, 950, 1000], THRESHOLD);
    assert_eq!(at, Some(1000));
}

#[test]
fn returns_last_timestamp_regardless_of_input_order() {
    let a = detect_recent_burst(&[1000, 0, 950, 100, 900], THRESHOLD).unwrap();
    let b = detect_recent_burst(&[0, 100, 900, 950, 1000], THRESHOLD).unwrap();
    assert_eq!(a, b, "RFC-001: order-invariant");
    assert_eq!(a, 1000);
}

#[test]
fn threshold_is_strict_not_inclusive() {
    // Exactly-at-threshold should NOT fire (we use `>` not `>=`).
    // 4 touches, threshold 0.5 → need >2 in window to fire. Here the
    // last quarter of span 0..400 is 300..400, containing 400 only → 1/4.
    assert_eq!(
        detect_recent_burst(&[0, 100, 200, 400], 0.25),
        None,
        "ratio == threshold must not fire",
    );
}
