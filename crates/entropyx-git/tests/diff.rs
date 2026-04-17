//! Tree-diff end-to-end: build a fixture whose commits span every
//! ChangeKind we detect (Added / Modified / Renamed / Deleted), then
//! assert `Repo::diff` returns the right kind per adjacent pair.

use entropyx_git::{ChangeKind, Repo};
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

/// Stage all changes and commit as a fixed author. Returns the commit SHA.
fn commit_all(cwd: &Path, subject: &str) -> String {
    run_git(cwd, &["add", "-A"]);
    run_git(
        cwd,
        &[
            "-c",
            "user.name=Fixture",
            "-c",
            "user.email=fixture@ex.com",
            "commit",
            "-q",
            "-m",
            subject,
        ],
    );
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(cwd)
        .output()
        .expect("rev-parse");
    String::from_utf8(out.stdout).unwrap().trim().to_string()
}

#[test]
fn diff_detects_add_modify_rename_delete() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    // c0: add a.txt + b.txt
    fs::write(
        root.join("a.txt"),
        "line 1\nline 2\nline 3\nline 4\nline 5\n",
    )
    .unwrap();
    fs::write(root.join("b.txt"), "beta\n").unwrap();
    let c0 = commit_all(root, "add a and b");

    // c1: modify a.txt (same path, different content)
    fs::write(
        root.join("a.txt"),
        "line 1\nline 2 changed\nline 3\nline 4\nline 5\n",
    )
    .unwrap();
    let c1 = commit_all(root, "modify a");

    // c2: rename a.txt -> c.txt with identical content
    fs::rename(root.join("a.txt"), root.join("c.txt")).unwrap();
    let c2 = commit_all(root, "rename a -> c");

    // c3: delete b.txt
    fs::remove_file(root.join("b.txt")).unwrap();
    let c3 = commit_all(root, "delete b");

    let repo = Repo::open(root).expect("open");

    // c0 -> c1: only a.txt is modified.
    let d01 = repo.diff(&c0, &c1).expect("diff 0->1");
    assert_eq!(d01.len(), 1);
    assert_eq!(d01[0].path, "a.txt");
    assert!(matches!(d01[0].kind, ChangeKind::Modified));

    // c1 -> c2: a.txt -> c.txt rename. b.txt untouched.
    let d12 = repo.diff(&c1, &c2).expect("diff 1->2");
    assert_eq!(d12.len(), 1, "exactly one rewrite, not add+delete pair");
    assert_eq!(d12[0].path, "c.txt");
    match &d12[0].kind {
        ChangeKind::Renamed { from, .. } => assert_eq!(from, "a.txt"),
        other => panic!("expected Renamed, got {other:?}"),
    }
    assert_eq!(d12[0].previous_path(), Some("a.txt"));

    // c2 -> c3: b.txt deleted. c.txt untouched.
    let d23 = repo.diff(&c2, &c3).expect("diff 2->3");
    assert_eq!(d23.len(), 1);
    assert_eq!(d23[0].path, "b.txt");
    assert!(matches!(d23[0].kind, ChangeKind::Deleted));
    assert_eq!(d23[0].previous_path(), Some("b.txt"));

    // c0 -> c0: no-op must produce empty diff (bitwise-stable guard).
    let same = repo.diff(&c0, &c0).expect("self-diff");
    assert!(same.is_empty());

    // Determinism: repeated call yields identical bytes (RFC-001).
    let d01_again = repo.diff(&c0, &c1).expect("diff 0->1 repeat");
    assert_eq!(d01, d01_again);
}
