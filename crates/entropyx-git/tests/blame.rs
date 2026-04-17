//! End-to-end blame: build a 2-commit fixture with lines added at
//! different times, call Repo::blame, and verify per-line attribution.

use entropyx_git::Repo;
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
fn blame_attributes_lines_to_their_commits() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);

    // c1: file starts with 2 lines, both authored at t=100.
    fs::write(root.join("f.rs"), "line one\nline two\n").unwrap();
    commit_as(root, "a@ex.com", 100, "c1");

    // c2: add a third line, preserving the first two. Third line authored at t=500.
    fs::write(root.join("f.rs"), "line one\nline two\nline three\n").unwrap();
    commit_as(root, "b@ex.com", 500, "c2");

    let repo = Repo::open(root).expect("open");
    let lines = repo.blame("f.rs").expect("blame");

    assert_eq!(lines.len(), 3, "three lines blamed");
    assert_eq!(lines[0].line_number, 1);
    assert_eq!(lines[1].line_number, 2);
    assert_eq!(lines[2].line_number, 3);

    // Lines 1+2 trace back to the same commit (c1 @ 100). Line 3 to c2.
    assert_eq!(lines[0].author_time, 100);
    assert_eq!(lines[1].author_time, 100);
    assert_eq!(lines[2].author_time, 500);
    assert_eq!(lines[0].commit_sha, lines[1].commit_sha);
    assert_ne!(lines[0].commit_sha, lines[2].commit_sha);
    assert_eq!(lines[0].commit_sha.len(), 40);
}

#[test]
fn blame_fails_on_missing_file() {
    let td = tempdir().expect("tempdir");
    let root = td.path();
    run_git(root, &["init", "--quiet"]);
    fs::write(root.join("x.rs"), "x\n").unwrap();
    commit_as(root, "a@ex.com", 100, "init");

    let repo = Repo::open(root).expect("open");
    let result = repo.blame("does-not-exist.rs");
    assert!(result.is_err(), "blame of missing file should error");
}
