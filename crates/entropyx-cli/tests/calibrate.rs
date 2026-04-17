//! End-to-end RFC-012 calibration loop:
//!   scan → summary.json
//!   labels.json (user-supplied)
//!   calibrate --summary --labels → weights.json
//!   scan --weights weights.json → different composite
//!
//! Asserts the commands compose cleanly and the fitted weights change
//! observable composite values.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

/// Build a Command rooted at the test binary with ENTROPYX_CACHE_DIR
/// pointing inside `td_path`, so each test gets isolated disk caches.
fn cli_cmd(td_path: &Path) -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_entropyx"));
    cmd.env("ENTROPYX_CACHE_DIR", td_path);
    cmd
}

fn run_git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

fn commit_as(cwd: &Path, email: &str, time: i64, subject: &str) {
    run_git(cwd, &["add", "-A"]);
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=T",
            "-c",
            &format!("user.email={email}"),
            "commit",
            "-q",
            "-m",
            subject,
        ])
        .env("GIT_AUTHOR_DATE", format!("@{time} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{time} +0000"))
        .current_dir(cwd)
        .status()
        .expect("spawn git");
    assert!(status.success(), "commit failed");
}

#[test]
fn calibrate_command_produces_valid_scoreweights() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    // 3 files with distinguishing activity so the feature rows aren't
    // all identical (which would make the fit degenerate).
    fs::write(root.join("hot.rs"), "pub fn a() {}\n").unwrap();
    fs::write(root.join("warm.rs"), "pub fn b() {}\n").unwrap();
    fs::write(root.join("stable.rs"), "pub fn c() {}\n").unwrap();
    commit_as(root, "alice@ex.com", 100, "init");

    fs::write(
        root.join("hot.rs"),
        "pub fn a() {}\npub fn a2() {}\npub fn a3() {}\n",
    )
    .unwrap();
    fs::write(root.join("warm.rs"), "pub fn b() {}\npub fn b2() {}\n").unwrap();
    commit_as(root, "bob@ex.com", 200, "expand hot + warm");

    fs::write(
        root.join("hot.rs"),
        "pub fn a() {}\npub fn a2() {}\npub fn a3() {}\npub fn a4() {}\n",
    )
    .unwrap();
    commit_as(root, "carol@ex.com", 400, "even more hot");

    // Step 1: scan → summary.json
    let scan_out = cli_cmd(td.path())
        .args(["scan"])
        .arg(root)
        .output()
        .expect("scan");
    assert!(
        scan_out.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&scan_out.stderr),
    );
    let summary_path = td.path().join("summary.json");
    fs::write(&summary_path, &scan_out.stdout).unwrap();

    // Step 2: write labels — user's ground truth for each file.
    let labels = serde_json::json!({
        "hot.rs": 0.9,
        "warm.rs": 0.5,
        "stable.rs": 0.1,
    });
    let labels_path = td.path().join("labels.json");
    fs::write(&labels_path, serde_json::to_string_pretty(&labels).unwrap()).unwrap();

    // Step 3: calibrate.
    let cal_out = cli_cmd(td.path())
        .args(["calibrate"])
        .arg("--summary")
        .arg(&summary_path)
        .arg("--labels")
        .arg(&labels_path)
        .output()
        .expect("calibrate");
    assert!(
        cal_out.status.success(),
        "calibrate failed: {}",
        String::from_utf8_lossy(&cal_out.stderr),
    );
    let weights: serde_json::Value = serde_json::from_slice(&cal_out.stdout).expect("weights JSON");

    // RFC-012 invariant: positives sum to 1.0.
    let positives: f64 = [
        "theta_d", "theta_h", "theta_v", "theta_c", "theta_b", "theta_s",
    ]
    .iter()
    .map(|k| weights[k].as_f64().unwrap())
    .sum();
    assert!(
        (positives - 1.0).abs() < 1e-9,
        "positives sum = {positives}",
    );
    // theta_t in [0, 1].
    let theta_t = weights["theta_t"].as_f64().unwrap();
    assert!(
        (0.0..=1.0).contains(&theta_t),
        "theta_t = {theta_t} out of range",
    );

    // Step 4: write weights, re-scan with them, and verify composite
    // values are finite (sanity — the weights are a valid input).
    let weights_path = td.path().join("weights.json");
    fs::write(&weights_path, &cal_out.stdout).unwrap();

    let scan2_out = cli_cmd(td.path())
        .args(["scan"])
        .arg(root)
        .arg("--weights")
        .arg(&weights_path)
        .output()
        .expect("scan with weights");
    assert!(
        scan2_out.status.success(),
        "weighted scan failed: {}",
        String::from_utf8_lossy(&scan2_out.stderr),
    );
    let summary2: serde_json::Value =
        serde_json::from_slice(&scan2_out.stdout).expect("weighted summary");
    for row in summary2["files"].as_array().unwrap() {
        let composite = row["values"][7].as_f64().unwrap();
        assert!(
            composite.is_finite(),
            "weighted composite {composite} not finite"
        );
    }
}

#[test]
fn scan_weights_flag_changes_composite() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("f.rs"), "pub fn a() {}\n").unwrap();
    commit_as(root, "a@ex.com", 100, "init");
    fs::write(root.join("f.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
    commit_as(root, "a@ex.com", 200, "add b");

    // Default scan.
    let default_out = cli_cmd(td.path())
        .args(["scan"])
        .arg(root)
        .output()
        .unwrap();
    assert!(default_out.status.success());
    let default: serde_json::Value = serde_json::from_slice(&default_out.stdout).unwrap();
    let default_composite = default["files"][0]["values"][7].as_f64().unwrap();

    // Custom weights: push all mass onto S_n, no discount. For a file
    // with S_n=1.0 this yields composite=1.0, distinguishable from the
    // default DEFAULT_WEIGHTS outcome.
    let weights = serde_json::json!({
        "theta_d": 0.0,
        "theta_h": 0.0,
        "theta_v": 0.0,
        "theta_c": 0.0,
        "theta_b": 0.0,
        "theta_s": 1.0,
        "theta_t": 0.0,
    });
    let weights_path = td.path().join("w.json");
    fs::write(&weights_path, serde_json::to_string(&weights).unwrap()).unwrap();

    let weighted_out = cli_cmd(td.path())
        .args(["scan"])
        .arg(root)
        .arg("--weights")
        .arg(&weights_path)
        .output()
        .unwrap();
    assert!(weighted_out.status.success());
    let weighted: serde_json::Value = serde_json::from_slice(&weighted_out.stdout).unwrap();
    let weighted_composite = weighted["files"][0]["values"][7].as_f64().unwrap();

    // All-on-S_n with S_n=1.0 → composite must be exactly 1.0.
    assert_eq!(
        weighted_composite, 1.0,
        "all-S weights should yield composite=1.0"
    );
    // And it differs from default.
    assert!(
        (weighted_composite - default_composite).abs() > 0.1,
        "weighted ({weighted_composite}) should differ meaningfully from default ({default_composite})",
    );
}

#[test]
fn scan_weights_rejects_malformed_json() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "fn x(){}").unwrap();
    commit_as(root, "a@ex.com", 100, "init");

    let bogus = td.path().join("bad.json");
    fs::write(&bogus, "{ not valid json").unwrap();

    let out = cli_cmd(td.path())
        .args(["scan"])
        .arg(root)
        .arg("--weights")
        .arg(&bogus)
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("invalid weights JSON"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn calibrate_empty_overlap_falls_back_to_defaults() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("foo.rs"), "pub fn a(){}").unwrap();
    commit_as(root, "a@ex.com", 100, "init");

    let scan_out = cli_cmd(td.path())
        .args(["scan"])
        .arg(root)
        .output()
        .unwrap();
    let summary_path = td.path().join("summary.json");
    fs::write(&summary_path, &scan_out.stdout).unwrap();

    // Labels map references a path that doesn't exist in the summary.
    let labels = serde_json::json!({ "nonexistent/file.rs": 0.9 });
    let labels_path = td.path().join("labels.json");
    fs::write(&labels_path, serde_json::to_string(&labels).unwrap()).unwrap();

    let cal_out = cli_cmd(td.path())
        .args(["calibrate"])
        .arg("--summary")
        .arg(&summary_path)
        .arg("--labels")
        .arg(&labels_path)
        .output()
        .unwrap();
    assert!(cal_out.status.success(), "calibrate should still succeed");
    // Warning goes to stderr, but the output weights must still be valid.
    let w: serde_json::Value = serde_json::from_slice(&cal_out.stdout).unwrap();
    assert!(
        (w["theta_d"].as_f64().unwrap() - 0.15).abs() < 1e-9,
        "empty overlap → DEFAULT_WEIGHTS (theta_d=0.15)",
    );
}
