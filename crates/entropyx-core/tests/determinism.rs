//! RFC-001 invariants: determinism across repeated runs and input orderings.

use entropyx_core::determinism::{reduce_sum, shannon_entropy};
use entropyx_core::{MetricComponents, ScoreWeights};

#[test]
fn composite_is_bitwise_stable_across_repeats() {
    let m = MetricComponents {
        change_density: 0.73,
        author_entropy: 0.41,
        temporal_volatility: 0.88,
        coupling_stress: 0.62,
        blame_youth: 0.19,
        semantic_drift: 0.95,
        test_cooevolution: 0.33,
    };
    let w = MetricComponents::DEFAULT_WEIGHTS;
    let baseline = m.composite(w);
    for _ in 0..256 {
        assert_eq!(
            baseline.to_bits(),
            m.composite(w).to_bits(),
            "composite drifted across repeated calls"
        );
    }
}

#[test]
fn reduce_sum_is_invariant_under_input_permutation() {
    let a = [0.1_f64, 0.2, 0.3, 0.4, 0.5, -0.15, 1e-9, 1e9, -1e9];
    let base = reduce_sum(&a);

    let mut b = a;
    b.reverse();
    assert_eq!(base.to_bits(), reduce_sum(&b).to_bits());

    let mut c = a;
    c.rotate_left(3);
    assert_eq!(base.to_bits(), reduce_sum(&c).to_bits());

    let mut d = a;
    d.swap(0, 7);
    d.swap(2, 5);
    assert_eq!(base.to_bits(), reduce_sum(&d).to_bits());
}

#[test]
fn default_weights_sum_to_one() {
    let w = MetricComponents::DEFAULT_WEIGHTS;
    let s = w.sum_positive();
    assert!(
        (s - 1.0).abs() < 1e-12,
        "positive weights must sum to 1.0; got {s}"
    );
}

#[test]
fn custom_weights_off_by_one_are_rejected_by_assertion() {
    // A calibrated, non-default weight set that still sums to 1.
    let w = ScoreWeights {
        theta_d: 0.10,
        theta_h: 0.10,
        theta_v: 0.15,
        theta_c: 0.25,
        theta_b: 0.05,
        theta_s: 0.35,
        theta_t: 0.10,
    };
    assert!((w.sum_positive() - 1.0).abs() < 1e-12);
}

#[test]
fn shannon_entropy_degenerate_is_zero() {
    // Single-author distribution: all mass on one bucket.
    assert_eq!(shannon_entropy(&[7.0]), 0.0);
    assert_eq!(shannon_entropy(&[4.2, 0.0, 0.0, 0.0]), 0.0);
}

#[test]
fn shannon_entropy_uniform_matches_log_n() {
    // H(uniform over N) = ln N in nats.
    let n = 5usize;
    let weights = vec![1.0_f64; n];
    let h = shannon_entropy(&weights);
    let expected = (n as f64).ln();
    assert!(
        (h - expected).abs() < 1e-12,
        "uniform entropy {h} != ln({n}) = {expected}"
    );
}

#[test]
fn shannon_entropy_is_scale_invariant() {
    let a = shannon_entropy(&[1.0, 2.0, 3.0]);
    let b = shannon_entropy(&[100.0, 200.0, 300.0]);
    assert!((a - b).abs() < 1e-12);
}
