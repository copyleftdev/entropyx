//! End-to-end CLI test: compiled `entropyx scan <tempdir>` emits a
//! tq1 Summary whose dict + FileRow values reflect a controlled
//! fixture. Analytic assertions — not loose ranges — so regressions
//! in the pipeline show up immediately.

use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn run_git(cwd: &Path, args: &[&str]) {
    let status = Command::new("git")
        .args(args)
        .current_dir(cwd)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git {args:?} failed");
}

fn commit_as(cwd: &Path, name: &str, email: &str, time: i64, subject: &str) {
    run_git(cwd, &["add", "-A"]);
    let status = Command::new("git")
        .args([
            "-c",
            &format!("user.name={name}"),
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
    assert!(status.success(), "git commit {subject} failed");
}

#[test]
fn scan_emits_tq1_summary() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    // c1 alice: add a.rs
    fs::write(root.join("a.rs"), "one\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add a");

    // c2 bob: modify a.rs + add b.rs (a,b co-change)
    fs::write(root.join("a.rs"), "one\ntwo\n").unwrap();
    fs::write(root.join("b.rs"), "bb\n").unwrap();
    commit_as(root, "Bob", "bob@ex.com", 200, "touch a, add b");

    // c3 alice: modify b.rs
    fs::write(root.join("b.rs"), "bb\ncc\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 400, "touch b");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn entropyx");
    assert!(
        out.status.success(),
        "scan failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");

    // --- Protocol envelope ---
    assert_eq!(summary["schema"]["name"], "tq1");
    assert_eq!(summary["schema"]["version"], "0.1.0");
    assert_eq!(summary["dict"]["metrics"].as_array().unwrap().len(), 8);
    assert_eq!(
        summary["dict"]["metrics"][0], "change_density",
        "RFC-007 column order is load-bearing",
    );
    assert_eq!(summary["dict"]["metrics"][7], "composite");

    // --- Dict contents ---
    let files: Vec<&str> = summary["dict"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["a.rs", "b.rs"]);

    let authors: Vec<&str> = summary["dict"]["authors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(authors.len(), 2);
    assert!(authors.contains(&"alice@ex.com"));
    assert!(authors.contains(&"bob@ex.com"));

    // --- Per-file rows ---
    let rows = summary["files"].as_array().unwrap();
    assert_eq!(rows.len(), 2);

    // rows are path-sorted (BTreeMap iteration); rows[0] = a.rs, rows[1] = b.rs.
    let a = &rows[0];
    let b = &rows[1];
    let a_vals: Vec<f64> = a["values"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();
    let b_vals: Vec<f64> = b["values"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect();

    // Shared invariants across both files.
    for v in [&a_vals, &b_vals] {
        assert_eq!(v.len(), 8);
        assert_eq!(v[0], 1.0, "D_n: both files touched twice → tied max");
        assert!((v[1] - 1.0).abs() < 1e-12, "H_a: 2 authors uniform");
        assert_eq!(v[2], 0.0, "V_t: 1 gap → variance of 1 sample = 0");
        assert_eq!(v[3], 1.0, "C_s: both co-changed in c2 → tied max");
        assert_eq!(v[5], 0.0, "S_n (AST layer not scaffolded)");
        assert_eq!(v[6], 0.0, "T_c (tests subsystem not scaffolded)");
    }

    // Per-file B_y divergence. Repo span is 100..400; quarter window is
    // [325, 400]. Git blame on a.rs at HEAD: line 1 ("one") → c1@100,
    // line 2 ("two") → c2@200 — both pre-window. For b.rs: line 1
    // ("bb") → c2@200, line 2 ("cc") → c3@400 — the latter is in-window.
    assert_eq!(a_vals[4], 0.0, "a.rs blame-youth: 0 of 2 lines recent");
    assert_eq!(b_vals[4], 0.5, "b.rs blame-youth: 1 of 2 lines recent");

    // Composite under DEFAULT_WEIGHTS diverges by the θ_b · B_y term:
    //   a: 0.15·1 + 0.15·1 + 0.10·0 + 0.20·1 + 0.10·0.0 = 0.50
    //   b: 0.15·1 + 0.15·1 + 0.10·0 + 0.20·1 + 0.10·0.5 = 0.55
    assert!((a_vals[7] - 0.50).abs() < 1e-12, "a composite = {}", a_vals[7]);
    assert!((b_vals[7] - 0.55).abs() < 1e-12, "b composite = {}", b_vals[7]);

    for row in [a, b] {
        assert_eq!(row["lineage_confidence"], 1.0);
        // Both satisfy RFC-008 OwnershipFragmentation (H_a=1, D_n=1).
        assert_eq!(row["signal_class"], "ownership_fragmentation");
    }

    // No renames in this fixture → events empty.
    assert!(summary["events"].as_array().unwrap().is_empty());

    // Both files exist at HEAD → both mint `file:<prefix>` handles.
    let handles = summary["handles"].as_object().unwrap();
    assert_eq!(handles.len(), 2, "one handle per HEAD blob");
    for (key, handle) in handles {
        assert!(key.starts_with("file:"), "handle key is `file:<prefix>`");
        assert_eq!(key.len(), "file:".len() + 12, "12-char blob prefix");
        assert_eq!(handle["kind"], "file");
        assert_eq!(
            handle["blob_prefix"].as_str().unwrap().len(),
            12,
            "blob prefix is 12 hex chars",
        );
    }
}

#[test]
fn scan_emits_rename_event() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("old.rs"), "original\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add old");

    // Rename old.rs -> new.rs with identical content.
    fs::rename(root.join("old.rs"), root.join("new.rs")).unwrap();
    commit_as(root, "Alice", "alice@ex.com", 200, "rename");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn entropyx");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let events = summary["events"].as_array().expect("events");
    assert_eq!(events.len(), 1, "exactly one rename event");

    let ev = &events[0];
    assert_eq!(ev["kind"], "rename");
    assert_eq!(ev["from"], "old.rs");
    assert_eq!(ev["to"], "new.rs");
    assert_eq!(ev["at"], 200);
    // `file` is the FileId of the *new* path, which is interned in the
    // BTreeMap walk order. Both paths exist across the walk but only
    // new.rs is in the final dict, at index 0 (sorted alphabetically
    // "new.rs" comes after "old.rs" — but interning order follows
    // BTreeMap key iteration, so old.rs gets 0, new.rs gets 1).
    let new_fid = ev["file"].as_u64().unwrap();
    let files = summary["dict"]["files"].as_array().unwrap();
    assert_eq!(files[new_fid as usize], "new.rs");

    // Rename event carries the SHA of the rename commit. Used to join
    // with Summary.enrichments.pull_requests under --github.
    let sha = ev["sha"].as_str().expect("sha field");
    assert_eq!(sha.len(), 40, "full 40-char SHA of rename commit");

    // Only new.rs exists at HEAD → exactly one handle. The deleted
    // old.rs has no current blob to hash, so no handle is minted.
    let handles = summary["handles"].as_object().unwrap();
    assert_eq!(handles.len(), 1, "renamed-away file has no handle");
    let (only_key, _) = handles.iter().next().unwrap();
    assert!(only_key.starts_with("file:"));
}

#[test]
fn scan_emits_hotspot_on_recent_burst() {
    // 5 commits over a span where 3 are clustered in the last quarter.
    // Ratios: last quarter = 750..1000, contains 900/950/1000 → 3/5 = 60%.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    let schedule = [(0, "v1"), (100, "v2"), (900, "v3"), (950, "v4"), (1000, "v5")];
    for (i, (time, content)) in schedule.iter().enumerate() {
        fs::write(root.join("hot.rs"), format!("{content}\n")).unwrap();
        commit_as(
            root,
            "Author",
            "author@ex.com",
            *time,
            &format!("commit {i}"),
        );
    }

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn entropyx");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");

    let hotspots: Vec<&serde_json::Value> = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "hotspot")
        .collect();

    assert_eq!(hotspots.len(), 1, "exactly one hotspot for the bursting file");
    let h = hotspots[0];
    assert_eq!(h["at"], 1000, "event time is the latest touch");
    assert_eq!(h["reason"], "recent_burst");
    assert_eq!(
        h["sha"].as_str().unwrap().len(),
        40,
        "hotspot event carries SHA of latest touch",
    );

    let files = summary["dict"]["files"].as_array().unwrap();
    let fid = h["file"].as_u64().unwrap() as usize;
    assert_eq!(files[fid], "hot.rs");
}

#[test]
fn scan_steady_cadence_emits_no_hotspot() {
    // Equal-spacing → ~25% in the last quarter → under threshold 0.5.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    for (i, t) in [100, 200, 300, 400, 500, 600].iter().enumerate() {
        fs::write(root.join("steady.rs"), format!("v{i}\n")).unwrap();
        commit_as(root, "Author", "author@ex.com", *t, &format!("c{i}"));
    }

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn entropyx");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let hotspots: Vec<&serde_json::Value> = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "hotspot")
        .collect();
    assert!(
        hotspots.is_empty(),
        "steady cadence must not trip hotspot rule",
    );
}

#[test]
fn scan_semantic_drift_distinguishes_api_change_from_cosmetic() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    // c1: two files, both start with a single public fn.
    fs::write(root.join("a.rs"), "pub fn one() {}\n").unwrap();
    fs::write(root.join("b.rs"), "pub fn only() {}\n").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "init");

    // c2: a.rs gets a real API addition; b.rs only gets whitespace + comment.
    fs::write(
        root.join("a.rs"),
        "pub fn one() {}\npub fn two() {}\npub fn three() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("b.rs"),
        "pub fn only() {\n    // added a comment, no API change\n}\n",
    )
    .unwrap();
    commit_as(root, "B", "b@ex.com", 200, "expand a, comment b");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let files: Vec<&str> = summary["dict"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["a.rs", "b.rs"]);

    let rows = summary["files"].as_array().unwrap();
    let a_sn = rows[0]["values"][5].as_f64().unwrap();
    let b_sn = rows[1]["values"][5].as_f64().unwrap();

    // a.rs gained 2 public fns (delta=2) across its walk; b.rs had only
    // cosmetic changes (delta=0). After unit_normalize, a.rs → 1.0
    // (max), b.rs → 0.0.
    //
    // Note: root-commit Added entries also contribute — a.rs starts with
    // 1 pub fn, b.rs starts with 1 pub fn. So raw totals are:
    //   a: 1 (root Add) + 2 (pub fn additions) = 3
    //   b: 1 (root Add) + 0 = 1
    // Normalized: a=1.0, b=1/3.
    assert!((a_sn - 1.0).abs() < 1e-12, "a.rs S_n = {a_sn}");
    assert!((b_sn - 1.0 / 3.0).abs() < 1e-12, "b.rs S_n = {b_sn}");

    // Classification picks up the S_n signal. a.rs has S_n=1.0 and
    // H_a=1.0 (two authors), so the new `ApiDrift` rule fires before
    // OwnershipFragmentation. b.rs has S_n=0.333, below the S_n
    // threshold, so it stays OwnershipFragmentation.
    assert_eq!(rows[0]["signal_class"], "api_drift");
    assert_eq!(rows[1]["signal_class"], "ownership_fragmentation");

    // Event::ApiDrift should also fire for a.rs carrying the raw API
    // delta count (1 root-commit add + 2 new pub fns = 3).
    let api_drifts: Vec<&serde_json::Value> = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "api_drift")
        .collect();
    assert_eq!(api_drifts.len(), 1);
    let ev = api_drifts[0];
    assert_eq!(ev["pub_items_changed"], 3);
    assert_eq!(ev["at"], 200, "latest touch time for a.rs");
    assert_eq!(
        ev["sha"].as_str().unwrap().len(),
        40,
        "api_drift carries SHA of latest touch",
    );
    let files = summary["dict"]["files"].as_array().unwrap();
    assert_eq!(files[ev["file"].as_u64().unwrap() as usize], "a.rs");
}

#[test]
fn scan_emits_incident_aftershock() {
    // 4 commits touching hot.rs with increasing gaps (V_t > 0.3) and one
    // `fix:` subject in the middle → IncidentAftershock should fire.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    let schedule = [
        (100, "initial state", "v1"),
        (300, "feat: add widget", "v2"),
        (700, "fix: null deref in hot path", "v3"),
        (1500, "follow-up cleanup", "v4"),
    ];
    for (t, subject, body) in schedule {
        fs::write(root.join("hot.rs"), format!("{body}\n")).unwrap();
        commit_as(root, "Author", "a@ex.com", t, subject);
    }

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let events = summary["events"].as_array().unwrap();

    let aftershocks: Vec<&serde_json::Value> = events
        .iter()
        .filter(|e| e["kind"] == "incident_aftershock")
        .collect();
    assert_eq!(aftershocks.len(), 1, "one aftershock for hot.rs");

    let ev = aftershocks[0];
    assert_eq!(ev["at"], 700, "event time is the incident commit");
    assert_eq!(
        ev["window_days"], 0,
        "single incident → zero-day window",
    );
    let files = summary["dict"]["files"].as_array().unwrap();
    assert_eq!(files[ev["file"].as_u64().unwrap() as usize], "hot.rs");

    // Classification overridden by aftershock.
    let rows = summary["files"].as_array().unwrap();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["signal_class"], "incident_aftershock");
}

#[test]
fn scan_no_incident_without_fix_subject() {
    // Same cadence, but no fix/hotfix/revert subject → no aftershock.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    let schedule = [(100, "v1"), (300, "v2"), (700, "v3"), (1500, "v4")];
    for (t, body) in schedule {
        fs::write(root.join("calm.rs"), format!("{body}\n")).unwrap();
        commit_as(root, "Author", "a@ex.com", t, "chore: bump");
    }

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let aftershocks: Vec<&serde_json::Value> = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "incident_aftershock")
        .collect();
    assert!(aftershocks.is_empty(), "no fix commits → no aftershock");
}

#[test]
fn scan_test_coevolution_discounts_well_tested_code() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::create_dir_all(root.join("src")).unwrap();
    fs::create_dir_all(root.join("tests")).unwrap();

    // c1: src/lib.rs alone (no test co-evolution).
    fs::write(root.join("src/lib.rs"), "pub fn one() {}\n").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "init lib");

    // c2: src/lib.rs + tests/lib_test.rs together (co-evolution).
    fs::write(root.join("src/lib.rs"), "pub fn one() {}\npub fn two() {}\n").unwrap();
    fs::write(root.join("tests/lib_test.rs"), "#[test] fn t() {}\n").unwrap();
    commit_as(root, "A", "a@ex.com", 200, "add feature + test");

    // c3: src/lib.rs + tests/lib_test.rs together again.
    fs::write(
        root.join("src/lib.rs"),
        "pub fn one() {}\npub fn two() {}\npub fn three() {}\n",
    )
    .unwrap();
    fs::write(
        root.join("tests/lib_test.rs"),
        "#[test] fn t() {}\n#[test] fn t2() {}\n",
    )
    .unwrap();
    commit_as(root, "A", "a@ex.com", 300, "extend + test");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");

    // Path-sorted dict: src/lib.rs=0, tests/lib_test.rs=1.
    let files: Vec<&str> = summary["dict"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["src/lib.rs", "tests/lib_test.rs"]);

    let rows = summary["files"].as_array().unwrap();
    let lib = &rows[0];
    let test = &rows[1];

    // src/lib.rs: 3 touches, 2 of which co-touched a test → T_c = 2/3.
    let lib_tc = lib["values"][6].as_f64().unwrap();
    assert!((lib_tc - 2.0 / 3.0).abs() < 1e-12, "lib T_c = {lib_tc}");

    // tests/lib_test.rs: test file gets T_c = 1.0 by convention.
    let test_tc = test["values"][6].as_f64().unwrap();
    assert_eq!(test_tc, 1.0);

    // Composite discount: θ_t = 0.05 so lib's discount is -0.05 * 2/3 ≈ -0.0333.
    // Without T_c, composite would be: D·0.15 + H·0.15 + V·0.10 + C·0.20
    //   + B·0.10 + S·0.30. With T_c, subtract 0.05·t_c.
    let expected_lib = 0.15 * lib["values"][0].as_f64().unwrap()
        + 0.15 * lib["values"][1].as_f64().unwrap()
        + 0.10 * lib["values"][2].as_f64().unwrap()
        + 0.20 * lib["values"][3].as_f64().unwrap()
        + 0.10 * lib["values"][4].as_f64().unwrap()
        + 0.30 * lib["values"][5].as_f64().unwrap()
        - 0.05 * lib_tc;
    let actual_lib = lib["values"][7].as_f64().unwrap();
    assert!(
        (actual_lib - expected_lib).abs() < 1e-10,
        "composite {actual_lib} ≠ expected {expected_lib}",
    );
}

#[test]
fn scan_computes_semantic_drift_for_go_files() {
    // Mixed-language fixture: one Rust file gets API expanded, one Go
    // file gets a new exported function. S_n should fire on both via
    // their respective backends.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("lib.rs"), "pub fn one() {}\n").unwrap();
    fs::write(root.join("main.go"), "package main\nfunc One() {}\n").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "init");

    fs::write(root.join("lib.rs"), "pub fn one() {}\npub fn two() {}\n").unwrap();
    fs::write(
        root.join("main.go"),
        "package main\nfunc One() {}\nfunc Two() {}\n",
    )
    .unwrap();
    commit_as(root, "A", "a@ex.com", 200, "extend both");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let files: Vec<&str> = summary["dict"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["lib.rs", "main.go"]);

    let rows = summary["files"].as_array().unwrap();
    let rust_sn = rows[0]["values"][5].as_f64().unwrap();
    let go_sn = rows[1]["values"][5].as_f64().unwrap();

    // Both files: root-commit add (1 public item) + c2 add (1 public
    // item) = 2 raw delta each. Normalized to tied max → both 1.0.
    assert_eq!(rust_sn, 1.0, "Rust S_n should fire on API expansion");
    assert_eq!(go_sn, 1.0, "Go S_n should fire via tree-sitter backend");
}

#[test]
fn scan_computes_semantic_drift_for_python_and_typescript() {
    // Polyglot fixture: a .py and a .ts file each gain one public item
    // between two commits. Both should register non-zero S_n.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("app.py"), "def one():\n    pass\n").unwrap();
    fs::write(
        root.join("index.ts"),
        "export function one(): void {}\n",
    )
    .unwrap();
    commit_as(root, "A", "a@ex.com", 100, "init");

    fs::write(
        root.join("app.py"),
        "def one():\n    pass\ndef two():\n    pass\n",
    )
    .unwrap();
    fs::write(
        root.join("index.ts"),
        "export function one(): void {}\nexport function two(): void {}\n",
    )
    .unwrap();
    commit_as(root, "A", "a@ex.com", 200, "expand both");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let files: Vec<&str> = summary["dict"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["app.py", "index.ts"]);

    let rows = summary["files"].as_array().unwrap();
    assert_eq!(rows[0]["values"][5], 1.0, "python S_n fires via tree-sitter");
    assert_eq!(rows[1]["values"][5], 1.0, "typescript S_n fires via tree-sitter");
}

#[test]
fn scan_lineage_collapses_renamed_file_history() {
    // A file named `lib.rs` is created in c1, modified in c2, renamed
    // to `core.rs` in c3, then modified again in c4. Without lineage
    // the trajectory fragments into two FileRows (lib.rs + core.rs).
    // With RFC-004 lineage they collapse into ONE row keyed on the
    // canonical name (core.rs, the newest).
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("lib.rs"), "v1\n").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "c1 create");

    fs::write(root.join("lib.rs"), "v1\nv2\n").unwrap();
    commit_as(root, "A", "a@ex.com", 200, "c2 modify");

    fs::rename(root.join("lib.rs"), root.join("core.rs")).unwrap();
    commit_as(root, "A", "a@ex.com", 300, "c3 rename");

    fs::write(root.join("core.rs"), "v1\nv2\nv3\n").unwrap();
    commit_as(root, "A", "a@ex.com", 400, "c4 modify");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&out.stderr),
    );

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");

    // Exactly one trajectory in the dict, canonicalized to the NEW name.
    let files: Vec<&str> = summary["dict"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["core.rs"], "pre-rename name collapsed away");

    // Exactly one FileRow with D_n=1.0 (it was the only file touched,
    // so it's tied at the normalization max) and all four commit times
    // aggregated under the canonical key.
    let rows = summary["files"].as_array().unwrap();
    assert_eq!(rows.len(), 1, "renamed file yields one row, not two");

    // The Rename event still fires, with `from`/`to` carrying the
    // literal filenames from the rename commit and `file` pointing to
    // the canonical FileRow.
    let renames: Vec<&serde_json::Value> = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "rename")
        .collect();
    assert_eq!(renames.len(), 1);
    assert_eq!(renames[0]["from"], "lib.rs");
    assert_eq!(renames[0]["to"], "core.rs");
    assert_eq!(
        renames[0]["file"].as_u64().unwrap(),
        0,
        "event's file FileId resolves to the single trajectory row",
    );
}

#[test]
fn scan_bridge_file_surfaces_via_betweenness() {
    // Topology: file `bridge.rs` is a bridge between two otherwise-
    // disconnected files `a.rs` and `c.rs`. bridge.rs co-changes with
    // a.rs in c1 and with c.rs in c2; a.rs and c.rs never co-change.
    //
    // Graph:     a.rs — bridge.rs — c.rs
    // Weighted degrees: a=1, bridge=2, c=1 → normalized: a=0.5, bridge=1.0, c=0.5
    // Betweenness:      a=0, bridge=1.0, c=0
    // With C_s = max: a=0.5, bridge=1.0, c=0.5 (same as degree-only here)
    //
    // The important check: betweenness actually fires in the output
    // rather than being silently dropped. We verify the bridge gets a
    // strictly larger C_s than either leaf, and that a.rs's value does
    // not get inflated by phantom betweenness (should be exactly 0.5).
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("a.rs"), "v1\n").unwrap();
    fs::write(root.join("bridge.rs"), "v1\n").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "init a + bridge");

    fs::write(root.join("bridge.rs"), "v2\n").unwrap();
    fs::write(root.join("c.rs"), "v1\n").unwrap();
    commit_as(root, "A", "a@ex.com", 200, "bridge + c");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let files: Vec<&str> = summary["dict"]["files"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["a.rs", "bridge.rs", "c.rs"]);

    let rows = summary["files"].as_array().unwrap();
    let cs_a = rows[0]["values"][3].as_f64().unwrap();
    let cs_bridge = rows[1]["values"][3].as_f64().unwrap();
    let cs_c = rows[2]["values"][3].as_f64().unwrap();

    // Degree-only normalization would give a=0.5, bridge=1.0, c=0.5.
    // Betweenness gives bridge=1.0, others=0. max preserves those.
    assert_eq!(cs_bridge, 1.0, "bridge node tops C_s");
    assert_eq!(cs_a, 0.5, "leaf exactly at its degree-only value");
    assert_eq!(cs_c, 0.5, "leaf exactly at its degree-only value");
    assert!(cs_bridge > cs_a && cs_bridge > cs_c, "bridge strictly exceeds leaves");
}

#[test]
fn scan_ignores_unsupported_languages() {
    // A Ruby file has no parser yet in v0.1 — contributes 0 to S_n.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("app.rb"), "def foo; end\n").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "init");
    fs::write(
        root.join("app.rb"),
        "def foo; end\ndef bar; end\n",
    )
    .unwrap();
    commit_as(root, "A", "a@ex.com", 200, "add bar");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let sn = summary["files"][0]["values"][5].as_f64().unwrap();
    assert_eq!(sn, 0.0, "unsupported language → S_n = 0");
}

#[test]
fn scan_emits_ownership_split_event() {
    // Alice solo-owns foo.rs for two commits, then Bob joins at c3.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("foo.rs"), "v1\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "alice c1");
    fs::write(root.join("foo.rs"), "v2\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 200, "alice c2");
    fs::write(root.join("foo.rs"), "v3\n").unwrap();
    commit_as(root, "Bob", "bob@ex.com", 300, "bob joins");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());

    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let splits: Vec<&serde_json::Value> = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "ownership_split")
        .collect();
    assert_eq!(splits.len(), 1, "one ownership split event");

    let ev = splits[0];
    assert_eq!(ev["at"], 300, "split fires at Bob's first commit");

    // Authors serialize as AuthorId integers (VertexTable indices). The
    // emails live in dict.authors at those positions.
    let author_ids: Vec<u64> = ev["authors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_u64().unwrap())
        .collect();
    let dict_authors: Vec<&str> = summary["dict"]["authors"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    let resolved: Vec<&str> = author_ids
        .iter()
        .map(|&i| dict_authors[i as usize])
        .collect();
    assert_eq!(resolved.len(), 2);
    assert!(resolved.contains(&"alice@ex.com"));
    assert!(resolved.contains(&"bob@ex.com"));

    let files = summary["dict"]["files"].as_array().unwrap();
    assert_eq!(files[ev["file"].as_u64().unwrap() as usize], "foo.rs");

    // Every event carries the SHA of its originating commit.
    assert_eq!(
        ev["sha"].as_str().unwrap().len(),
        40,
        "ownership split carries SHA of the split commit",
    );
}

#[test]
fn scan_no_split_for_single_author_file() {
    // Alice alone, no split ever possible.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    for (i, t) in [100, 200, 300].iter().enumerate() {
        fs::write(root.join("solo.rs"), format!("v{i}\n")).unwrap();
        commit_as(root, "Alice", "alice@ex.com", *t, &format!("c{i}"));
    }
    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let summary: serde_json::Value =
        serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    let splits: Vec<_> = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|e| e["kind"] == "ownership_split")
        .collect();
    assert!(splits.is_empty(), "single author → no split");
}

#[test]
fn scan_incident_aftershock_event_carries_sha_field() {
    // Regression: Event::IncidentAftershock gained a `sha` field so
    // consumers can join it to Summary.enrichments.pull_requests. Even
    // without --github, the SHA must be populated.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    let schedule = [
        (100, "chore: init", "v1"),
        (300, "feat: add widget", "v2"),
        (700, "fix: crash in hot path", "v3"),
        (1500, "follow-up cleanup", "v4"),
    ];
    for (t, subject, body) in schedule {
        fs::write(root.join("hot.rs"), format!("{body}\n")).unwrap();
        commit_as(root, "A", "a@ex.com", t, subject);
    }

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin).args(["scan"]).arg(root).output().unwrap();
    assert!(out.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let aftershock = summary["events"]
        .as_array()
        .unwrap()
        .iter()
        .find(|e| e["kind"] == "incident_aftershock")
        .expect("aftershock event");
    let sha = aftershock["sha"].as_str().expect("sha field present");
    assert_eq!(sha.len(), 40, "full 40-char SHA");

    // No --github → enrichments empty.
    let prs = summary["enrichments"]["pull_requests"]
        .as_object()
        .expect("enrichments.pull_requests exists");
    assert!(prs.is_empty(), "no --github → no PR entries");
}

#[test]
fn scan_github_auto_detect_fails_when_no_remote() {
    // Fixture has no `origin` remote configured. Bare `--github`
    // should error with the expected message, not panic.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "fn x(){}").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "init");

    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan"])
        .arg(root)
        .arg("--github")
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("auto-detect"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn scan_on_nonexistent_path_fails_cleanly() {
    let bin = env!("CARGO_BIN_EXE_entropyx");
    let out = Command::new(bin)
        .args(["scan", "/definitely/not/a/repo/x7f2"])
        .output()
        .expect("spawn entropyx");
    assert!(!out.status.success(), "expected failure");
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("open failed"),
        "stderr should explain the failure: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}
