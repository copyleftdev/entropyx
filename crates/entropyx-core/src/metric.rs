//! Composite entropy scoring (RFC-007) and signal taxonomy (RFC-008).
//!
//! Every emission carries the full decomposition — consumers, human or AI,
//! never receive a bare scalar. Weights are a *hypothesis* calibrated per
//! repo via the ridge-regression harness described in RFC-007.

use crate::determinism::{reduce_sum, shannon_entropy};
use crate::id::{FileId, LineageConfidence};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

/// The six sub-fields of the composite, plus the test-coevolution discount.
/// Each is normalized to `[0, 1]` per analysis window before composition.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct MetricComponents {
    /// Change density D_n — energy injected into the file.
    pub change_density: f64,
    /// Authorship dispersion H_a — Shannon entropy over contribution mass.
    pub author_entropy: f64,
    /// Temporal volatility V_t — burstiness (coefficient of variation of
    /// inter-arrival gaps between modification events).
    pub temporal_volatility: f64,
    /// Coupling stress C_s — weighted centrality in the co-change graph.
    pub coupling_stress: f64,
    /// Blame youth B_y — fraction of lines whose last-touch is recent.
    pub blame_youth: f64,
    /// Semantic drift S_n — AST / public-API / control-flow delta (RFC-005).
    pub semantic_drift: f64,
    /// Test co-evolution T_c — discount term; higher is *better*.
    pub test_cooevolution: f64,
}

impl MetricComponents {
    /// RFC-007 v1 default weights. Semantic delta dominates (θ_s = 0.30);
    /// test co-evolution discounts (θ_t = 0.05) so frozen-neglect files are
    /// not rewarded for being stable.
    pub const DEFAULT_WEIGHTS: ScoreWeights = ScoreWeights {
        theta_d: 0.15,
        theta_h: 0.15,
        theta_v: 0.10,
        theta_c: 0.20,
        theta_b: 0.10,
        theta_s: 0.30,
        theta_t: 0.05,
    };

    /// Composite score. The positive terms are summed through
    /// `reduce_sum` for determinism (RFC-001); the discount is applied after.
    pub fn composite(self, w: ScoreWeights) -> f64 {
        let positive = [
            w.theta_d * self.change_density,
            w.theta_h * self.author_entropy,
            w.theta_v * self.temporal_volatility,
            w.theta_c * self.coupling_stress,
            w.theta_b * self.blame_youth,
            w.theta_s * self.semantic_drift,
        ];
        let base = reduce_sum(&positive);
        let discount = w.theta_t * self.test_cooevolution;
        base - discount
    }
}

/// Convex combination weights θ for `MetricComponents::composite`.
/// Positive-term weights must sum to 1.0 (ULP tolerance) — enforced by the
/// `weights_sum_to_one` invariant test (RFC-012).
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ScoreWeights {
    pub theta_d: f64,
    pub theta_h: f64,
    pub theta_v: f64,
    pub theta_c: f64,
    pub theta_b: f64,
    pub theta_s: f64,
    pub theta_t: f64,
}

impl ScoreWeights {
    /// Sum of the six positive-term weights (excluding the T_c discount).
    pub fn sum_positive(&self) -> f64 {
        reduce_sum(&[
            self.theta_d,
            self.theta_h,
            self.theta_v,
            self.theta_c,
            self.theta_b,
            self.theta_s,
        ])
    }
}

/// Per-file metric row with full decomposition, lineage confidence, and
/// optional signal classification.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Metric {
    pub file: FileId,
    pub components: MetricComponents,
    pub composite: f64,
    pub lineage_confidence: LineageConfidence,
    pub signal_class: Option<SignalClass>,
}

/// Shannon entropy (nats) of an author-contribution distribution.
///
/// Input is a bag of author identifiers (typically email addresses — the
/// identity layer lives in `entropyx-github`, RFC-010; this function
/// treats strings as opaque). Histogramming is deterministic via
/// `BTreeMap`; the reduction is deterministic via `reduce_sum`. Returns
/// 0 for an empty bag or a single-author distribution.
pub fn author_entropy_nats<S: AsRef<str>>(authors: &[S]) -> f64 {
    if authors.is_empty() {
        return 0.0;
    }
    let mut counts: BTreeMap<&str, f64> = BTreeMap::new();
    for a in authors {
        *counts.entry(a.as_ref()).or_insert(0.0) += 1.0;
    }
    let weights: Vec<f64> = counts.into_values().collect();
    shannon_entropy(&weights)
}

/// Author dispersion normalized to `[0, 1]`: H_a / ln(n_distinct).
///
/// Maps "single owner" → 0 and "uniform over N distinct authors" → 1.
/// Feeds the `H_a` slot of `MetricComponents` directly. Returns 0 for
/// zero or one distinct author.
pub fn author_dispersion<S: AsRef<str>>(authors: &[S]) -> f64 {
    let distinct: BTreeSet<&str> = authors.iter().map(|a| a.as_ref()).collect();
    let n = distinct.len();
    if n < 2 {
        return 0.0;
    }
    author_entropy_nats(authors) / (n as f64).ln()
}

/// Raw per-file change counts (RFC-007 `D_n` numerator): how many
/// commits in the window touched each path. Input is one entry per
/// commit; inner `Vec<String>` is the set of paths changed in that
/// commit. Deterministic via `BTreeMap`.
pub fn change_counts<S: AsRef<str>>(per_commit_paths: &[Vec<S>]) -> BTreeMap<String, u64> {
    let mut counts: BTreeMap<String, u64> = BTreeMap::new();
    for paths in per_commit_paths {
        for p in paths {
            *counts.entry(p.as_ref().to_string()).or_insert(0) += 1;
        }
    }
    counts
}

/// Monotonic squash of an unbounded non-negative quantity into `[0, 1)`.
/// `saturate_unit(0) = 0`, `saturate_unit(1) = 0.5`, and the function
/// asymptotes to 1 as `x → ∞`.
///
/// Purpose: make unbounded statistical quantities like `V_t`
/// (coefficient of variation) composable with weighted-sum composites
/// that assume `[0, 1]`-scaled inputs. Preserves ranking (monotonic),
/// unlike a hard `min(x, 1)` clip which loses resolution above 1.
/// Negative inputs get mapped via `|x|` so the result is never
/// negative — callers that can produce signed values should pre-handle
/// the sign intentionally.
pub fn saturate_unit(x: f64) -> f64 {
    let a = x.abs();
    a / (1.0 + a)
}

/// Max-normalize raw counts into `[0, 1]`. Empty input yields empty
/// output; a map whose values are all zero yields all-zero output
/// (division by zero is short-circuited, not panicked).
pub fn unit_normalize(raw: &BTreeMap<String, u64>) -> BTreeMap<String, f64> {
    let max = raw.values().copied().max().unwrap_or(0);
    if max == 0 {
        return raw.keys().map(|k| (k.clone(), 0.0)).collect();
    }
    let max_f = max as f64;
    raw.iter()
        .map(|(k, v)| (k.clone(), (*v as f64) / max_f))
        .collect()
}

/// Temporal volatility (RFC-007 `V_t`): coefficient of variation of
/// inter-arrival gaps between event timestamps.
///
/// Input is unix epoch seconds; order does not matter (the function
/// sorts internally). Returns 0 for fewer than 2 events, or when every
/// gap is zero (all events collide in time). High values indicate
/// bursty activity; low values indicate steady cadence.
pub fn temporal_volatility(times: &[i64]) -> f64 {
    if times.len() < 2 {
        return 0.0;
    }
    let mut sorted: Vec<i64> = times.to_vec();
    sorted.sort_unstable();
    let gaps: Vec<f64> = sorted.windows(2).map(|w| (w[1] - w[0]) as f64).collect();
    let n = gaps.len() as f64;
    let mean = reduce_sum(&gaps) / n;
    if mean <= 0.0 {
        return 0.0;
    }
    let devs: Vec<f64> = gaps.iter().map(|g| (g - mean).powi(2)).collect();
    let variance = reduce_sum(&devs) / n;
    variance.sqrt() / mean
}

/// Configuration for RFC-012 weight calibration.
///
/// Defaults are calibrated-for-calibration: enough iterations and a
/// small-enough learning rate that unit tests recover `DEFAULT_WEIGHTS`
/// from synthetic labels to within 1e-3 on ~200 samples.
#[derive(Copy, Clone, Debug)]
pub struct CalibrationConfig {
    /// L2 regularization strength. `0.0` gives ordinary least squares
    /// (exact recovery on clean data); small positive values (≈0.01)
    /// are preferred for noisy real-world labels.
    pub lambda: f64,
    /// Gradient-descent iterations.
    pub iterations: usize,
    /// Learning rate. 0.1 is stable for [0,1]-scaled features.
    pub learning_rate: f64,
}

impl Default for CalibrationConfig {
    fn default() -> Self {
        Self {
            lambda: 0.0,
            iterations: 20_000,
            learning_rate: 0.1,
        }
    }
}

/// RFC-012 weight calibration: fit `ScoreWeights` from per-file feature
/// vectors and ground-truth label scores via ridge regression (gradient
/// descent with L2 regularization).
///
/// Feature layout per row: `[D_n, H_a, V_t, C_s, B_y, S_n, T_c]`.
/// Label in `[0, 1]` is the consumer's notion of ground-truth trouble
/// for the file — typically drawn from incident history, change-failure
/// rate, or human labeling.
///
/// **Sign convention**: the raw 7-parameter fit solves
/// `y ≈ θ_0·D + … + θ_5·S + θ_6·T`. `T_c` enters the composite as a
/// *discount*, so a well-fitting calibration produces `θ_6 < 0`. The
/// post-process step negates and clamps it into `θ_t ∈ [0, 1]`.
///
/// **Normalization**: the six positive-term weights are clamped to
/// `≥ 0` (a negative best-fit is semantically "no contribution") and
/// rescaled so their sum is exactly `1.0`, enforcing the RFC-012
/// invariant. Callers that want the raw unconstrained vector can
/// subtract the constraint-projection step manually.
///
/// Returns `DEFAULT_WEIGHTS` when the input is empty or degenerate —
/// safe fallback rather than a panic.
pub fn calibrate(features: &[[f64; 7]], labels: &[f64], config: CalibrationConfig) -> ScoreWeights {
    let n = features.len();
    if n == 0 || n != labels.len() {
        return MetricComponents::DEFAULT_WEIGHTS;
    }

    let mut theta = [0.0_f64; 7];
    let n_f = n as f64;

    for _ in 0..config.iterations {
        let mut grad = [0.0_f64; 7];
        for (x, &y) in features.iter().zip(labels.iter()) {
            let pred: f64 = (0..7).map(|i| theta[i] * x[i]).sum();
            let err = pred - y;
            for i in 0..7 {
                grad[i] += err * x[i] / n_f;
            }
        }
        for i in 0..7 {
            grad[i] += config.lambda * theta[i];
            theta[i] -= config.learning_rate * grad[i];
        }
    }

    // Project onto the RFC-012 feasible region: positives ≥ 0 summing to
    // 1, `theta_t` as the negated discount.
    let mut positives = [
        theta[0].max(0.0),
        theta[1].max(0.0),
        theta[2].max(0.0),
        theta[3].max(0.0),
        theta[4].max(0.0),
        theta[5].max(0.0),
    ];
    let sum: f64 = positives.iter().sum();
    if sum > 0.0 {
        for p in &mut positives {
            *p /= sum;
        }
    } else {
        // All positives clamped to zero — fall back to DEFAULT to avoid
        // emitting an invariant-violating weights set.
        return MetricComponents::DEFAULT_WEIGHTS;
    }
    let theta_t = (-theta[6]).clamp(0.0, 1.0);

    ScoreWeights {
        theta_d: positives[0],
        theta_h: positives[1],
        theta_v: positives[2],
        theta_c: positives[3],
        theta_b: positives[4],
        theta_s: positives[5],
        theta_t,
    }
}

/// Detect an ownership split: the moment a file that was clearly owned
/// by one author gains a new contributor. Returns `Some((split_time,
/// distinct_authors))` on a hit; `None` when the pattern doesn't apply.
///
/// Rule (v0.1 calibration):
///   - file has at least 3 chronologically-ordered touches
///   - the *first* author held ≥2 consecutive leading commits (clear
///     ownership established, not just a one-off)
///   - the next commit is by a different author (the split moment)
///
/// The caller supplies pairs sorted by time ascending; the function
/// does not sort internally so callers can reuse sorted data and catch
/// ordering bugs upstream.
pub fn detect_ownership_split<'a>(
    pairs_chrono: &'a [(i64, &'a str)],
) -> Option<(i64, Vec<&'a str>)> {
    if pairs_chrono.len() < 3 {
        return None;
    }
    let first_author = pairs_chrono[0].1;
    let leading = pairs_chrono
        .iter()
        .take_while(|(_, a)| *a == first_author)
        .count();
    if leading < 2 {
        return None;
    }
    if leading >= pairs_chrono.len() {
        return None; // no split — still single-owner
    }
    let split_time = pairs_chrono[leading].0;
    let mut unique: BTreeSet<&str> = BTreeSet::new();
    for (_, a) in pairs_chrono {
        unique.insert(a);
    }
    Some((split_time, unique.into_iter().collect()))
}

/// True if a path names a test file by common conventions.
///
/// v0.1 detection is path-shape only and language-agnostic:
///   - any `tests/` segment (root or nested — matches `crates/x/tests/y.rs`)
///   - file ending in `_test.rs` / `_tests.rs` / `_spec.rs`
///   - file literally named `tests.rs`
///
/// Content-based detection (`#[cfg(test)] mod tests {…}` inside a source
/// file) is out of scope for v0.1 — it needs a parser and would require
/// promoting every source scan to an AST pass. When `entropyx-ast` grows
/// richer, revisit this.
pub fn is_test_path(path: &str) -> bool {
    let normalized = path.replace('\\', "/");
    // Directory-level conventions across ecosystems:
    //   Rust/Go/Java/generic: `tests/`
    //   JS/TS/Python/Ruby:    `test/`, `__tests__/`, `spec/`
    for dir in ["tests", "test", "__tests__", "spec"] {
        let prefix = format!("{dir}/");
        let mid = format!("/{prefix}");
        if normalized.starts_with(&prefix) || normalized.contains(&mid) {
            return true;
        }
    }
    let fname = normalized.rsplit('/').next().unwrap_or("");
    // Single-name test modules (Rust: `tests.rs`; Python: `tests.py`).
    if fname == "tests.rs" || fname == "tests.py" || fname == "test.py" {
        return true;
    }
    // Filename-level conventions by ecosystem:
    //   Rust:   `*_test.rs`, `*_tests.rs`, `*_spec.rs`
    //   Go:     `*_test.go`
    //   JS/TS:  `*.test.(js|jsx|ts|tsx|mjs|cjs)`, `*.spec.(...)`
    //   Python: `test_*.py`, `*_test.py`
    //   Ruby:   `*_spec.rb`, `*_test.rb`
    //   Java:   `*Test.java`, `*Tests.java`, `*Spec.java`
    //   C++:    `*_test.cc|cpp|cxx|hpp|h`, `*_tests.cc|cpp|...`
    let suffixes = [
        "_test.rs",
        "_tests.rs",
        "_spec.rs",
        "_test.go",
        ".test.js",
        ".test.jsx",
        ".test.ts",
        ".test.tsx",
        ".test.mjs",
        ".test.cjs",
        ".spec.js",
        ".spec.jsx",
        ".spec.ts",
        ".spec.tsx",
        ".spec.mjs",
        ".spec.cjs",
        "_test.py",
        "_tests.py",
        "_spec.rb",
        "_test.rb",
        "Test.java",
        "Tests.java",
        "Spec.java",
        "_test.cc",
        "_test.cpp",
        "_test.cxx",
        "_test.hpp",
        "_test.h",
        "_tests.cc",
        "_tests.cpp",
        "_tests.cxx",
    ];
    if suffixes.iter().any(|suf| fname.ends_with(suf)) {
        return true;
    }
    // Python `test_*.py` prefix convention — prefix-based, not suffix.
    if fname.starts_with("test_") && fname.ends_with(".py") {
        return true;
    }
    false
}

/// True if a commit subject conventionally indicates an incident
/// response. Matches `fix`/`hotfix`/`revert` prefixes in the shapes
/// used by conventional-commits (`fix:`, `fix(scope):`, `fix!:`,
/// `fix <msg>`). Case-insensitive. Does NOT match `fixup` (git-autosquash
/// commits aren't incidents) or `prefix` (not an incident marker).
pub fn is_incident_subject(subject: &str) -> bool {
    let s = subject.trim_start().to_ascii_lowercase();
    // Every keyword is matched as a prefix ONLY when followed by a
    // conventional-commits separator. This rejects `fixup` (autosquash),
    // `revertable` (adjective), `hotfixed` (past tense), etc.
    for prefix in ["fix", "hotfix", "revert"] {
        if let Some(rest) = s.strip_prefix(prefix)
            && matches!(rest.chars().next(), Some(':' | ' ' | '(' | '!'))
        {
            return true;
        }
    }
    false
}

/// RFC-007 `B_y`: "blame youth" — fraction of a file's lines whose
/// last-touch time falls in the most-recent quarter of the repository
/// time span `[first, last]`. Maps "wall of fresh edits" → 1.0 and
/// "deep sediment of old code" → 0.0.
///
/// `first` and `last` are the repository-wide bounds (from the walk)
/// so per-file values are comparable across the tq1 output. Passing
/// the same bounds to every file is load-bearing — using per-file
/// bounds would collapse every file into a self-relative window and
/// erase cross-file comparisons.
///
/// Returns 0.0 on empty input or degenerate spans (`last <= first`).
pub fn blame_youth(line_times: &[i64], first: i64, last: i64) -> f64 {
    if line_times.is_empty() {
        return 0.0;
    }
    let span = last - first;
    if span <= 0 {
        return 0.0;
    }
    let window_start = last - (span / 4).max(1);
    let in_window = line_times.iter().filter(|&&t| t >= window_start).count();
    in_window as f64 / line_times.len() as f64
}

/// Detect a "recent burst" hotspot for a single file given its
/// modification timestamps.
///
/// The rule: if more than `threshold` of the file's touches fall within
/// the most-recent quarter of its observed time span, the file is
/// entering (or in) a disturbance window. Returns the timestamp of
/// the most recent touch as the event time; returns `None` when the
/// rule does not fire or the input is too sparse to be meaningful.
///
/// **Why "quarter"**: small enough that a constant-cadence file does
/// not trigger (1-in-4 touches fall in the last quarter by definition,
/// so threshold > 0.25 rejects steady activity), wide enough that a
/// handful of bunched touches at the end reliably trip it. Swap for
/// RFC-007 calibrated thresholds when RFC-012 lands.
///
/// **Determinism**: the function sorts inputs internally so call order
/// does not affect output (RFC-001).
pub fn detect_recent_burst(times: &[i64], threshold: f64) -> Option<i64> {
    if times.len() < 3 {
        return None;
    }
    let mut sorted = times.to_vec();
    sorted.sort_unstable();
    let first = *sorted.first().unwrap();
    let last = *sorted.last().unwrap();
    let span = last - first;
    if span <= 0 {
        return None;
    }
    // Integer-divide the span; the `min(1)` guard avoids a zero-width
    // window when a very short span rounds down to 0.
    let window_span = (span / 4).max(1);
    let window_start = last - window_span;
    let in_window = sorted.iter().filter(|&&t| t >= window_start).count();
    let ratio = in_window as f64 / sorted.len() as f64;
    if ratio > threshold { Some(last) } else { None }
}

/// RFC-008 classifier: rule-based, deterministic assignment of a
/// `SignalClass` from a file's `MetricComponents`. Returns `None` when
/// no rule fires — consumers should treat absence as "unremarkable".
///
/// `IncidentAftershock` is still deliberately withheld — it needs a
/// commit-subject scan for `fix:`/`hotfix` markers which isn't wired.
/// Emitting it prematurely would lie about what we observed.
///
/// Thresholds below are v0.1 calibration; they will migrate to a
/// per-repo ridge-regression harness once RFC-012 lands.
pub fn classify(m: &MetricComponents) -> Option<SignalClass> {
    let d = m.change_density;
    let h = m.author_entropy;
    let v = m.temporal_volatility;
    let c = m.coupling_stress;
    let s = m.semantic_drift;

    // Order matters: most specific first so a file that satisfies two
    // rules gets the sharper label.

    // CoupledAmplifier — a small, rarely-touched file that many others
    // change with. Blast-radius risk.
    if d < 0.3 && c > 0.7 {
        return Some(SignalClass::CoupledAmplifier);
    }

    // RefactorConvergence — concentrated ownership pushing meaningful
    // public-API change. "Planned redesign" — the S_n is driven by one
    // or two authors, not the whole team.
    if s > 0.6 && h < 0.4 {
        return Some(SignalClass::RefactorConvergence);
    }

    // ApiDrift — meaningful public-API change with diffuse ownership.
    // No one owner is steering the shift, so the interface rots under
    // many uncoordinated hands. (When T_c comes online this rule will
    // additionally require low test co-evolution — "silent" rot.)
    if s > 0.6 && h >= 0.4 {
        return Some(SignalClass::ApiDrift);
    }

    // OwnershipFragmentation — meaningful activity spread across many
    // authors. Bus-factor / coordination-drift risk. Sits below the S_n
    // rules so high-surface-change cases get ApiDrift first.
    if h > 0.8 && d > 0.3 {
        return Some(SignalClass::OwnershipFragmentation);
    }

    // FrozenNeglect — near-zero signal on every observable axis. Rot
    // hiding as stability. Also requires low volatility so we don't
    // misclassify a legitimately quiet, well-owned file.
    if d < 0.15 && h < 0.15 && v < 0.15 && c < 0.15 {
        return Some(SignalClass::FrozenNeglect);
    }

    None
}

/// RFC-008 taxonomy. Classification is rule-based and deterministic; no
/// ML models at v1 — they would break determinism and reproducibility.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum SignalClass {
    /// High S_n, falling H_a, rising T_c — planned redesign.
    RefactorConvergence,
    /// High public-API delta with low T_c growth — silent interface rot.
    ApiDrift,
    /// Rising H_a with stable D_n — team reorg / bus-factor risk.
    OwnershipFragmentation,
    /// Burst V_t clustered post-commit tagged `fix:`/`hotfix` — firefighting.
    IncidentAftershock,
    /// Low D_n, high C_s — small file with systemic blast radius.
    CoupledAmplifier,
    /// Low everything, old blame, no test co-evolution — rot hiding as stability.
    FrozenNeglect,
}
