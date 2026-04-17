//! Unit-saturation tests: the squash that makes unbounded statistical
//! measures composable with weighted-sum composites.

use entropyx_core::metric::saturate_unit;

#[test]
fn zero_maps_to_zero() {
    assert_eq!(saturate_unit(0.0), 0.0);
}

#[test]
fn one_maps_to_half() {
    assert!((saturate_unit(1.0) - 0.5).abs() < 1e-12);
}

#[test]
fn monotonic_preserves_ranking() {
    let xs = [0.1, 0.5, 1.0, 2.0, 5.0, 100.0];
    let ys: Vec<f64> = xs.iter().map(|&x| saturate_unit(x)).collect();
    for w in ys.windows(2) {
        assert!(w[0] < w[1], "monotonicity broken: {} !< {}", w[0], w[1]);
    }
}

#[test]
fn asymptotes_to_one_but_never_reaches_it() {
    assert!(saturate_unit(1e9) < 1.0);
    assert!(saturate_unit(1e9) > 0.999);
}

#[test]
fn negative_inputs_saturate_via_absolute_value() {
    // Signed values shouldn't yield negative output — callers that want
    // signed behavior should pre-sign intentionally.
    assert_eq!(saturate_unit(-1.0), 0.5);
    assert_eq!(saturate_unit(-5.0), saturate_unit(5.0));
}

#[test]
fn bounds_a_raw_coefficient_of_variation() {
    // Realistic scenario: a bursty file has CV in [0.5, 5.0] range.
    // After saturation it lives in [~0.33, ~0.83] — exactly what the
    // composite formula needs for a well-scaled V_t term.
    for cv in [0.5, 1.0, 2.0, 5.0, 10.0] {
        let s = saturate_unit(cv);
        assert!(s > 0.0 && s < 1.0, "saturate({cv}) = {s} out of (0, 1)");
    }
}
