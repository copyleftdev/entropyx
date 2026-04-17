//! RFC-007 `B_y`: blame youth fraction tests.

use entropyx_core::metric::blame_youth;

#[test]
fn empty_input_is_zero() {
    assert_eq!(blame_youth(&[], 0, 1000), 0.0);
}

#[test]
fn degenerate_span_is_zero() {
    assert_eq!(blame_youth(&[500], 1000, 500), 0.0, "last < first");
    assert_eq!(blame_youth(&[500], 500, 500), 0.0, "zero span");
}

#[test]
fn all_lines_recent_is_one() {
    // Span 0..1000 → window 750..1000. All lines fall in window → 1.0.
    let y = blame_youth(&[800, 900, 1000], 0, 1000);
    assert_eq!(y, 1.0);
}

#[test]
fn all_lines_old_is_zero() {
    // All times before window_start.
    let y = blame_youth(&[0, 100, 200, 300], 0, 1000);
    assert_eq!(y, 0.0);
}

#[test]
fn half_recent_half_old() {
    // 2 recent (800, 900) + 2 old (100, 200) → 0.5.
    let y = blame_youth(&[100, 200, 800, 900], 0, 1000);
    assert_eq!(y, 0.5);
}

#[test]
fn single_line_right_at_last_is_recent() {
    let y = blame_youth(&[1000], 0, 1000);
    assert_eq!(y, 1.0);
}

#[test]
fn order_invariant() {
    // RFC-001: filter-count is naturally order-independent, but assert
    // so future micro-optimizations don't silently break this.
    let a = blame_youth(&[100, 200, 800, 900], 0, 1000);
    let b = blame_youth(&[900, 200, 100, 800], 0, 1000);
    assert_eq!(a.to_bits(), b.to_bits());
}
