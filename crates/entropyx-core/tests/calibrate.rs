//! RFC-012 weight calibration: does the ridge fit recover known weights
//! from synthetic data, and does it degrade sensibly at the edges?

use entropyx_core::metric::{calibrate, CalibrationConfig, MetricComponents};
use entropyx_core::ScoreWeights;

/// Reproducible linear-congruential pseudorandom in [0, 1) so tests
/// don't depend on the `rand` crate or thread state.
fn lcg(seed: u64) -> impl FnMut() -> f64 {
    let mut s = seed;
    move || {
        s = s.wrapping_mul(6_364_136_223_846_793_005).wrapping_add(1);
        (s >> 33) as f64 / (1u64 << 31) as f64
    }
}

fn generate_features(n: usize, seed: u64) -> Vec<[f64; 7]> {
    let mut r = lcg(seed);
    (0..n)
        .map(|_| {
            let mut row = [0.0; 7];
            for v in &mut row {
                *v = r();
            }
            row
        })
        .collect()
}

fn compose(features: &[[f64; 7]], w: ScoreWeights) -> Vec<f64> {
    features
        .iter()
        .map(|x| {
            let mc = MetricComponents {
                change_density: x[0],
                author_entropy: x[1],
                temporal_volatility: x[2],
                coupling_stress: x[3],
                blame_youth: x[4],
                semantic_drift: x[5],
                test_cooevolution: x[6],
            };
            mc.composite(w)
        })
        .collect()
}

fn nearly_equal(a: ScoreWeights, b: ScoreWeights, tol: f64) {
    let diffs = [
        (a.theta_d - b.theta_d).abs(),
        (a.theta_h - b.theta_h).abs(),
        (a.theta_v - b.theta_v).abs(),
        (a.theta_c - b.theta_c).abs(),
        (a.theta_b - b.theta_b).abs(),
        (a.theta_s - b.theta_s).abs(),
        (a.theta_t - b.theta_t).abs(),
    ];
    let max: f64 = diffs.iter().copied().fold(0.0, f64::max);
    assert!(
        max <= tol,
        "weights differ by {max} > {tol}. got={a:?} want={b:?}",
    );
}

#[test]
fn recovers_default_weights_from_clean_synthetic_data() {
    let features = generate_features(300, 1);
    let labels = compose(&features, MetricComponents::DEFAULT_WEIGHTS);
    let fitted = calibrate(&features, &labels, CalibrationConfig::default());
    nearly_equal(fitted, MetricComponents::DEFAULT_WEIGHTS, 1e-3);
}

#[test]
fn recovers_alternative_weights() {
    // A plausible repo-calibrated weight set: heavier S_n dominance,
    // lighter D_n, stronger T_c discount.
    let truth = ScoreWeights {
        theta_d: 0.05,
        theta_h: 0.15,
        theta_v: 0.05,
        theta_c: 0.20,
        theta_b: 0.10,
        theta_s: 0.45,
        theta_t: 0.15,
    };
    let features = generate_features(300, 2);
    let labels = compose(&features, truth);
    let fitted = calibrate(&features, &labels, CalibrationConfig::default());
    nearly_equal(fitted, truth, 1e-3);
}

#[test]
fn positives_always_sum_to_one() {
    let features = generate_features(200, 3);
    let labels = compose(&features, MetricComponents::DEFAULT_WEIGHTS);
    let fitted = calibrate(&features, &labels, CalibrationConfig::default());
    let sum = fitted.sum_positive();
    assert!((sum - 1.0).abs() < 1e-9, "sum = {sum}");
}

#[test]
fn theta_t_is_non_negative_and_bounded() {
    let features = generate_features(200, 4);
    let labels = compose(&features, MetricComponents::DEFAULT_WEIGHTS);
    let fitted = calibrate(&features, &labels, CalibrationConfig::default());
    assert!(
        (0.0..=1.0).contains(&fitted.theta_t),
        "theta_t = {} outside [0, 1]",
        fitted.theta_t,
    );
}

#[test]
fn empty_input_returns_default() {
    let fitted = calibrate(&[], &[], CalibrationConfig::default());
    nearly_equal(fitted, MetricComponents::DEFAULT_WEIGHTS, 0.0);
}

#[test]
fn length_mismatch_returns_default() {
    let features = generate_features(5, 5);
    let labels = vec![0.5, 0.6];
    let fitted = calibrate(&features, &labels, CalibrationConfig::default());
    nearly_equal(fitted, MetricComponents::DEFAULT_WEIGHTS, 0.0);
}

#[test]
fn degenerate_all_zero_labels_returns_default() {
    // All labels zero → all positive weights fit to zero → projection
    // falls back to DEFAULT_WEIGHTS rather than emitting a zeroed
    // (invariant-violating) weight set.
    let features = generate_features(50, 6);
    let labels = vec![0.0; 50];
    let fitted = calibrate(&features, &labels, CalibrationConfig::default());
    nearly_equal(fitted, MetricComponents::DEFAULT_WEIGHTS, 0.0);
}

#[test]
fn regularization_shrinks_toward_uniform_positives() {
    // Under a strong ridge penalty with sparse signal, positives flatten.
    // We don't assert an exact value — just that high lambda produces a
    // different, more uniform distribution than lambda=0.
    let features = generate_features(200, 7);
    let labels = compose(&features, MetricComponents::DEFAULT_WEIGHTS);

    let loose = calibrate(
        &features,
        &labels,
        CalibrationConfig {
            lambda: 0.0,
            ..Default::default()
        },
    );
    let tight = calibrate(
        &features,
        &labels,
        CalibrationConfig {
            lambda: 0.5,
            ..Default::default()
        },
    );

    // Compute variance across positives for each.
    fn var(w: ScoreWeights) -> f64 {
        let vals = [w.theta_d, w.theta_h, w.theta_v, w.theta_c, w.theta_b, w.theta_s];
        let mean = vals.iter().sum::<f64>() / vals.len() as f64;
        vals.iter().map(|v| (v - mean).powi(2)).sum::<f64>() / vals.len() as f64
    }
    assert!(
        var(tight) < var(loose),
        "tight-regularization variance ({}) should be ≤ loose ({})",
        var(tight),
        var(loose),
    );
}
