//! Unit tests for the RFC-007 physics primitives: author dispersion and
//! temporal volatility. Determinism and boundary cases are load-bearing
//! (RFC-001, RFC-012).

use entropyx_core::metric::{author_dispersion, author_entropy_nats, temporal_volatility};

#[test]
fn author_entropy_empty_is_zero() {
    let empty: [&str; 0] = [];
    assert_eq!(author_entropy_nats(&empty), 0.0);
}

#[test]
fn author_entropy_single_author_is_zero() {
    assert_eq!(author_entropy_nats(&["alice", "alice", "alice"]), 0.0);
}

#[test]
fn author_entropy_uniform_over_two_is_ln2() {
    let h = author_entropy_nats(&["alice", "bob"]);
    assert!((h - std::f64::consts::LN_2).abs() < 1e-12, "got {h}");
}

#[test]
fn author_entropy_is_order_invariant() {
    let a = author_entropy_nats(&["a", "b", "a", "c", "b", "b"]);
    let b = author_entropy_nats(&["b", "b", "a", "a", "c", "b"]);
    assert_eq!(
        a.to_bits(),
        b.to_bits(),
        "RFC-001: bitwise-stable across order"
    );
}

#[test]
fn author_dispersion_single_owner_is_zero() {
    assert_eq!(author_dispersion(&["alice", "alice", "alice"]), 0.0);
}

#[test]
fn author_dispersion_uniform_is_one() {
    let d = author_dispersion(&["a", "b", "c", "d"]);
    assert!((d - 1.0).abs() < 1e-12, "uniform over N → 1, got {d}");
}

#[test]
fn author_dispersion_skewed_is_between_zero_and_one() {
    // 3 alice, 1 bob → dispersion well under 1.
    let d = author_dispersion(&["alice", "alice", "alice", "bob"]);
    assert!(d > 0.0 && d < 1.0, "skewed dispersion {d} not in (0, 1)");
}

#[test]
fn temporal_volatility_constant_cadence_is_zero() {
    assert_eq!(temporal_volatility(&[0, 10, 20, 30, 40]), 0.0);
}

#[test]
fn temporal_volatility_empty_and_single_are_zero() {
    assert_eq!(temporal_volatility(&[]), 0.0);
    assert_eq!(temporal_volatility(&[100]), 0.0);
}

#[test]
fn temporal_volatility_all_same_time_is_zero() {
    assert_eq!(temporal_volatility(&[100, 100, 100, 100]), 0.0);
}

#[test]
fn temporal_volatility_is_order_invariant() {
    let a = temporal_volatility(&[100, 200, 400, 800]);
    let b = temporal_volatility(&[800, 400, 200, 100]);
    assert_eq!(
        a.to_bits(),
        b.to_bits(),
        "RFC-001: bitwise-stable across order"
    );
}

#[test]
fn temporal_volatility_exponential_gaps_is_high() {
    // Gaps: 100, 200, 400, 800 — growing geometrically → high CV.
    let v = temporal_volatility(&[0, 100, 300, 700, 1500]);
    assert!(
        v > 0.5,
        "exponential spacing should yield CV > 0.5, got {v}"
    );
}
