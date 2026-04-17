//! RFC-008 classifier: each class fires on exactly the intended region
//! of the metric space, and anything benign stays unclassified.

use entropyx_core::SignalClass;
use entropyx_core::metric::{MetricComponents, classify};

fn mc(d: f64, h: f64, v: f64, c: f64) -> MetricComponents {
    mcs(d, h, v, c, 0.0)
}

fn mcs(d: f64, h: f64, v: f64, c: f64, s: f64) -> MetricComponents {
    MetricComponents {
        change_density: d,
        author_entropy: h,
        temporal_volatility: v,
        coupling_stress: c,
        blame_youth: 0.0,
        semantic_drift: s,
        test_cooevolution: 0.0,
    }
}

#[test]
fn coupled_amplifier_fires_on_low_d_high_c() {
    let m = mc(0.10, 0.20, 0.10, 0.90);
    assert_eq!(classify(&m), Some(SignalClass::CoupledAmplifier));
}

#[test]
fn ownership_fragmentation_fires_on_high_h_moderate_d() {
    let m = mc(0.60, 0.90, 0.20, 0.30);
    assert_eq!(classify(&m), Some(SignalClass::OwnershipFragmentation));
}

#[test]
fn frozen_neglect_fires_on_near_zero_everything() {
    let m = mc(0.05, 0.05, 0.05, 0.05);
    assert_eq!(classify(&m), Some(SignalClass::FrozenNeglect));
}

#[test]
fn moderate_activity_with_single_author_is_unclassified() {
    // Stable, well-owned, not amplifying. Shouldn't trip any v0.1 rule.
    let m = mc(0.50, 0.30, 0.20, 0.40);
    assert_eq!(classify(&m), None);
}

#[test]
fn high_activity_single_author_is_unclassified() {
    // Busy but owned. Not pathological in v0.1 rules.
    let m = mc(0.90, 0.10, 0.50, 0.40);
    assert_eq!(classify(&m), None);
}

#[test]
fn coupled_amplifier_beats_frozen_neglect_on_overlap() {
    // Low D and H, but C is high — more specific rule should win.
    let m = mc(0.05, 0.05, 0.05, 0.95);
    assert_eq!(classify(&m), Some(SignalClass::CoupledAmplifier));
}

#[test]
fn refactor_convergence_fires_on_high_s_low_h() {
    // One owner pushing a lot of API surface change — planned redesign.
    let m = mcs(0.80, 0.20, 0.30, 0.40, 0.90);
    assert_eq!(classify(&m), Some(SignalClass::RefactorConvergence));
}

#[test]
fn api_drift_fires_on_high_s_diffuse_ownership() {
    // High public-API delta, many hands on it — uncoordinated drift.
    let m = mcs(0.60, 0.90, 0.30, 0.40, 0.80);
    assert_eq!(classify(&m), Some(SignalClass::ApiDrift));
}

#[test]
fn high_s_beats_ownership_fragmentation() {
    // H=1.0 would trip OwnershipFragmentation, but S>0.6 lifts it to
    // ApiDrift — the sharper label when the surface is actually shifting.
    let m = mcs(0.60, 1.00, 0.30, 0.40, 0.70);
    assert_eq!(classify(&m), Some(SignalClass::ApiDrift));
}

#[test]
fn moderate_s_allows_ownership_fragmentation() {
    // Same H=1.0 and D=0.6 but S=0.2 — below RefactorConvergence /
    // ApiDrift threshold, so OwnershipFragmentation still applies.
    let m = mcs(0.60, 1.00, 0.30, 0.40, 0.20);
    assert_eq!(classify(&m), Some(SignalClass::OwnershipFragmentation));
}

#[test]
fn coupled_amplifier_still_beats_s_based_rules() {
    // Blast-radius risk should take priority over semantic-drift labels.
    let m = mcs(0.10, 0.20, 0.10, 0.90, 0.80);
    assert_eq!(classify(&m), Some(SignalClass::CoupledAmplifier));
}

#[test]
fn classify_is_deterministic_pure_function() {
    // Pure function: same input → same output. (Guard against accidental
    // introduction of global state or randomness later.)
    let m = mc(0.50, 0.90, 0.20, 0.30);
    let a = classify(&m);
    let b = classify(&m);
    let c = classify(&m);
    assert_eq!(a, b);
    assert_eq!(b, c);
}
