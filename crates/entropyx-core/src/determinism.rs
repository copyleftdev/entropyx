//! Deterministic numerical primitives (RFC-001).
//!
//! f64 addition is not associative; a parallel reduction with a nondeterministic
//! schedule produces different bit patterns on repeat. `reduce_sum` sidesteps
//! that by sorting by absolute magnitude descending before folding, which is
//! both deterministic across input orders *and* better-conditioned than naive
//! fold for heterogeneous magnitudes.
//!
//! If we later observe ULP-scale drift on real corpora that matters for
//! cross-run bitwise equality, swap the fold for Kahan-Neumaier compensated
//! summation while keeping the magnitude sort for ordering stability.

/// Sum `xs` deterministically. The result is a pure function of the
/// multiset of values, not of their input order.
pub fn reduce_sum(xs: &[f64]) -> f64 {
    let mut v: Vec<f64> = xs.to_vec();
    v.sort_by(|a, b| {
        debug_assert!(!a.is_nan() && !b.is_nan(), "NaN in deterministic sum");
        b.abs()
            .partial_cmp(&a.abs())
            .unwrap_or(std::cmp::Ordering::Equal)
    });
    v.into_iter().fold(0.0_f64, |acc, x| acc + x)
}

/// Clamp to `[0, 1]`. Anything outside is a bug upstream; debug builds
/// panic loudly, release builds clamp silently so one bad metric does not
/// sink the whole pipeline.
pub fn unit(x: f64) -> f64 {
    debug_assert!(
        x.is_finite() && (0.0..=1.0).contains(&x),
        "out-of-range unit value: {x}"
    );
    x.clamp(0.0, 1.0)
}

/// Shannon entropy of a discrete distribution, in nats (natural log).
/// Input must be non-negative and sum to a positive finite value; the
/// function normalizes internally, so callers need not pre-normalize.
///
/// Returns 0.0 for the degenerate single-support case, matching the limit
/// of `-p log p` as `p -> 1`.
pub fn shannon_entropy(weights: &[f64]) -> f64 {
    let total = reduce_sum(weights);
    debug_assert!(total.is_finite() && total > 0.0);
    if total <= 0.0 {
        return 0.0;
    }
    let terms: Vec<f64> = weights
        .iter()
        .map(|&w| {
            let p = w / total;
            if p > 0.0 { -p * p.ln() } else { 0.0 }
        })
        .collect();
    reduce_sum(&terms)
}
