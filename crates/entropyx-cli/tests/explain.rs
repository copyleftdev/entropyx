//! End-to-end: compiled `entropyx explain <repo> <file>` filters the
//! walk to the requested file and emits per-file evidence + commit list.

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
fn explain_filters_to_single_file() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    // 3 commits: a.rs touched in c1+c2; b.rs touched in c2+c3.
    fs::write(root.join("a.rs"), "one\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add a");

    fs::write(root.join("a.rs"), "one\ntwo\n").unwrap();
    fs::write(root.join("b.rs"), "bb\n").unwrap();
    commit_as(root, "Bob", "bob@ex.com", 200, "touch a, add b");

    fs::write(root.join("b.rs"), "bb\ncc\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 400, "touch b");

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("a.rs")
        .output()
        .expect("spawn entropyx");
    assert!(
        out.status.success(),
        "explain failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");

    assert_eq!(report["schema"]["name"], "entropyx-explain");
    assert_eq!(report["path"], "a.rs");
    assert_eq!(report["commits_touched"], 2);
    assert_eq!(report["first_commit_time"], 100);
    assert_eq!(report["last_commit_time"], 200);

    // Two commits, two distinct authors at 50% each.
    let tops = report["top_authors"].as_array().unwrap();
    assert_eq!(tops.len(), 2);
    for t in tops {
        assert!((t["share"].as_f64().unwrap() - 0.5).abs() < 1e-12);
    }

    // Commits list is newest-first (walk order), only the ones touching a.rs.
    let commits = report["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["subject"], "touch a, add b");
    assert_eq!(commits[0]["author"], "bob@ex.com");
    assert_eq!(commits[1]["subject"], "add a");
    assert_eq!(commits[1]["author"], "alice@ex.com");

    // Per-file metrics: 2 authors uniform → dispersion=1.0; 1 gap → V_t=0.
    let m = &report["metrics"];
    assert_eq!(m["change_count"], 2);
    assert!((m["author_dispersion"].as_f64().unwrap() - 1.0).abs() < 1e-12);
    assert_eq!(m["temporal_volatility"], 0.0);
}

#[test]
fn explain_unknown_path_yields_empty_evidence() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "one\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add a");

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("does/not/exist.rs")
        .output()
        .expect("spawn entropyx");
    assert!(out.status.success(), "unknown path is a valid empty query");

    let report: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is JSON");
    assert_eq!(report["commits_touched"], 0);
    assert!(report["commits"].as_array().unwrap().is_empty());
    assert!(report["top_authors"].as_array().unwrap().is_empty());
    assert!(report["first_commit_time"].is_null());
}

#[test]
fn explain_resolves_handle_key() {
    // Round-trip: `scan` mints a file:<prefix> key; feeding that key back
    // to `explain` must resolve to the same path and produce the same
    // per-file evidence as calling with the raw path.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("target.rs"), "v1\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add target");
    fs::write(root.join("target.rs"), "v1\nv2\n").unwrap();
    commit_as(root, "Bob", "bob@ex.com", 200, "touch target");

    // Step 1: scan emits a Summary; grab the handle key.
    let scan_out = cli_cmd(td.path())
        .args(["scan"])
        .arg(root)
        .output()
        .expect("scan");
    assert!(scan_out.status.success());
    let summary: serde_json::Value = serde_json::from_slice(&scan_out.stdout).unwrap();
    let handles = summary["handles"].as_object().unwrap();
    assert_eq!(handles.len(), 1);
    let handle_key = handles.keys().next().unwrap().clone();
    assert!(handle_key.starts_with("file:"));

    // Step 2: explain with the handle key.
    let explain_by_handle = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg(&handle_key)
        .output()
        .expect("explain");
    assert!(
        explain_by_handle.status.success(),
        "explain via handle: stderr={}",
        String::from_utf8_lossy(&explain_by_handle.stderr),
    );
    let by_handle: serde_json::Value = serde_json::from_slice(&explain_by_handle.stdout).unwrap();
    assert_eq!(by_handle["path"], "target.rs");
    assert_eq!(by_handle["commits_touched"], 2);

    // Step 3: explain with the raw path — should match exactly.
    let explain_by_path = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("target.rs")
        .output()
        .expect("explain");
    assert!(explain_by_path.status.success());
    let by_path: serde_json::Value = serde_json::from_slice(&explain_by_path.stdout).unwrap();
    assert_eq!(
        by_handle, by_path,
        "handle and path must yield identical evidence"
    );
}

#[test]
fn explain_unknown_handle_fails_cleanly() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "x\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add a");

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("file:deadbeefcafe")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("matches no blob at HEAD"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

fn rev_parse_head(cwd: &Path) -> String {
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .expect("rev-parse");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn explain_commit_handle_returns_meta_and_changes() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("a.rs"), "v1\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add a");
    fs::write(root.join("a.rs"), "v1\nv2\n").unwrap();
    fs::write(root.join("b.rs"), "bb\n").unwrap();
    commit_as(root, "Bob", "bob@ex.com", 200, "touch a, add b");

    let sha = rev_parse_head(root);
    assert_eq!(sha.len(), 40);

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg(format!("commit:{sha}"))
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "commit explain failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["kind"], "commit");
    assert_eq!(r["commit"]["sha"], sha);
    assert_eq!(r["commit"]["subject"], "touch a, add b");
    assert_eq!(r["commit"]["author"]["email"], "bob@ex.com");
    assert_eq!(r["commit"]["committer"]["time"], 200);
    assert_eq!(r["commit"]["parents"].as_array().unwrap().len(), 1);

    let changes = r["changes"].as_array().unwrap();
    assert_eq!(changes.len(), 2);
    let paths: Vec<&str> = changes
        .iter()
        .map(|c| c["path"].as_str().unwrap())
        .collect();
    assert!(paths.contains(&"a.rs"));
    assert!(paths.contains(&"b.rs"));
    assert_eq!(r["stats"]["files_changed"], 2);
    assert_eq!(r["stats"]["renames"], 0);
}

#[test]
fn explain_commit_unknown_sha_fails_cleanly() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "x\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add");

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("commit:0000000000000000000000000000000000000000")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("not found"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn explain_range_handle_excludes_base() {
    // 3 linear commits: c1 → c2 → c3. `range:c1..c3` should yield
    // {c2, c3} — the canonical git log base..head set.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("f.rs"), "v1\n").unwrap();
    commit_as(root, "A", "a@ex.com", 100, "c1");
    let c1 = rev_parse_head(root);

    fs::write(root.join("f.rs"), "v1\nv2\n").unwrap();
    commit_as(root, "B", "b@ex.com", 200, "c2");

    fs::write(root.join("f.rs"), "v1\nv2\nv3\n").unwrap();
    commit_as(root, "A", "a@ex.com", 300, "c3");
    let c3 = rev_parse_head(root);

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg(format!("range:{c1}..{c3}"))
        .output()
        .expect("spawn");
    assert!(
        out.status.success(),
        "range explain failed: stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(r["kind"], "range");
    assert_eq!(r["range"]["base"], c1);
    assert_eq!(r["range"]["head"], c3);
    assert_eq!(r["commit_count"], 2, "base is excluded");
    assert_eq!(r["distinct_authors"], 2);
    assert_eq!(r["first_commit_time"], 200);
    assert_eq!(r["last_commit_time"], 300);
    // The single file was touched in both c2 and c3, so exactly one entry.
    let files: Vec<&str> = r["files_touched"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    assert_eq!(files, vec!["f.rs"]);

    // Commits list is newest-first (matches walk order).
    let commits = r["commits"].as_array().unwrap();
    assert_eq!(commits.len(), 2);
    assert_eq!(commits[0]["subject"], "c3");
    assert_eq!(commits[1]["subject"], "c2");
}

#[test]
fn explain_commit_without_github_flag_has_no_pr_field() {
    // Regression guard: adding --github is strictly additive. When the
    // flag is absent the commit explain output must not contain
    // "pull_request" at all (not even null).
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "v1\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "init");
    let sha = rev_parse_head(root);

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg(format!("commit:{sha}"))
        .output()
        .expect("spawn");
    assert!(out.status.success());
    let r: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert!(r.get("pull_request").is_none(), "no flag → no field");
}

#[test]
fn explain_rejects_malformed_github_slug() {
    // With --github present the slug must contain exactly one '/'.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "v1\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "init");
    let sha = rev_parse_head(root);

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg(format!("commit:{sha}"))
        .arg("--github")
        .arg("not-a-valid-slug")
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("owner/name"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn explain_range_malformed_fails_cleanly() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("a.rs"), "x\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add");

    let out = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("range:deadbeef") // missing ".."
        .output()
        .expect("spawn");
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("malformed range"),
        "stderr: {}",
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn explain_follows_renames() {
    // A commit that renames a.rs -> c.rs should count as a "touch" for
    // BOTH paths (a.rs disappears; c.rs appears). The AI can ask about
    // either side of the rename and get the rewrite in its evidence.
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    fs::write(root.join("a.rs"), "original\n").unwrap();
    commit_as(root, "Alice", "alice@ex.com", 100, "add a");

    // Rename a.rs -> c.rs with no content change.
    fs::rename(root.join("a.rs"), root.join("c.rs")).unwrap();
    commit_as(root, "Alice", "alice@ex.com", 200, "rename a -> c");

    // Ask about the old path — should see 2 touches (add + rename-away).
    let out_old = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("a.rs")
        .output()
        .expect("spawn");
    assert!(out_old.status.success());
    let r_old: serde_json::Value = serde_json::from_slice(&out_old.stdout).unwrap();
    assert_eq!(r_old["commits_touched"], 2, "a.rs: add + rename-away");

    // Ask about the new path — should see 1 touch (rename-to).
    let out_new = cli_cmd(td.path())
        .args(["explain"])
        .arg(root)
        .arg("c.rs")
        .output()
        .expect("spawn");
    assert!(out_new.status.success());
    let r_new: serde_json::Value = serde_json::from_slice(&out_new.stdout).unwrap();
    assert_eq!(r_new["commits_touched"], 1, "c.rs: rename-to only");
}
