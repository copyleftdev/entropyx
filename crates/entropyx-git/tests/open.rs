//! Integration tests for the local collector. The fresh-init test proves
//! the facade links; the commit-fixture test proves we extract owned
//! `CommitMeta` from a real HEAD without leaking gix types.

use entropyx_git::Repo;
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

fn commit(cwd: &Path, subject: &str) {
    run_git(
        cwd,
        &[
            "-c", "user.name=Test",
            "-c", "user.email=test@example.com",
            "commit", "--allow-empty", "-q", "-m", subject,
        ],
    );
}

#[test]
fn opens_a_freshly_initialized_repo() {
    let td = tempdir().expect("tempdir");
    let _ = gix::init(td.path()).expect("gix init");

    let repo = Repo::open(td.path()).expect("open");
    assert!(repo.git_dir().ends_with(".git"));
    assert!(!repo.has_commits(), "fresh repo has no HEAD");
    assert!(
        repo.head_commit_meta().expect("head meta").is_none(),
        "unborn HEAD yields None, not error",
    );
}

#[test]
fn reads_head_commit_after_commit() {
    let td = tempdir().expect("tempdir");
    let _ = gix::init(td.path()).expect("gix init");

    // Use `-c` to inject identity so no repo-local or global config is
    // required. `--allow-empty` sidesteps the need to write a worktree
    // file in a scaffold test.
    commit(td.path(), "initial commit");

    let repo = Repo::open(td.path()).expect("open");
    assert!(repo.has_commits());

    let head = repo
        .head_commit_meta()
        .expect("head meta")
        .expect("HEAD resolves");

    assert_eq!(head.sha.len(), 40, "SHA-1 object id is 40 hex chars");
    assert!(head.parents.is_empty(), "root commit has no parents");
    assert_eq!(head.tree.len(), 40);
    assert_eq!(head.author.name, "Test");
    assert_eq!(head.author.email, "test@example.com");
    assert_eq!(head.committer.name, "Test");
    assert_eq!(head.subject, "initial commit");
    assert!(head.author.time > 0, "author time is a unix epoch");
}

#[test]
fn head_tree_entries_lists_blobs_sorted() {
    use std::fs;
    let td = tempdir().expect("tempdir");
    let root = td.path();
    let _ = gix::init(root).expect("init");

    fs::write(root.join("z.rs"), "z\n").unwrap();
    fs::write(root.join("a.rs"), "a\n").unwrap();
    fs::create_dir(root.join("src")).unwrap();
    fs::write(root.join("src/lib.rs"), "lib\n").unwrap();
    use std::process::Command;
    let status = Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .status()
        .expect("git add");
    assert!(status.success());
    commit(root, "initial");

    let repo = Repo::open(root).expect("open");
    let entries = repo.head_tree_entries().expect("entries");
    assert_eq!(entries.len(), 3);
    // Sorted by path:
    assert_eq!(entries[0].0, "a.rs");
    assert_eq!(entries[1].0, "src/lib.rs");
    assert_eq!(entries[2].0, "z.rs");
    // Each SHA is a full 40-char hex blob id.
    for (_, sha) in &entries {
        assert_eq!(sha.len(), 40);
    }
}

#[test]
fn head_tree_entries_empty_for_unborn_head() {
    let td = tempdir().expect("tempdir");
    let _ = gix::init(td.path()).expect("init");
    let repo = Repo::open(td.path()).expect("open");
    assert!(repo.head_tree_entries().expect("entries").is_empty());
}

#[test]
fn blob_at_reads_file_contents_at_specific_commit() {
    use std::fs;
    let td = tempdir().expect("tempdir");
    let root = td.path();
    let _ = gix::init(root).expect("init");

    fs::write(root.join("f.rs"), "v1\n").unwrap();
    use std::process::Command;
    Command::new("git").args(["add", "-A"]).current_dir(root).status().unwrap();
    commit(root, "c1");
    let c1 = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    let c1_sha = String::from_utf8(c1.stdout).unwrap().trim().to_string();

    fs::write(root.join("f.rs"), "v1\nv2\n").unwrap();
    Command::new("git").args(["add", "-A"]).current_dir(root).status().unwrap();
    commit(root, "c2");
    let c2 = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    let c2_sha = String::from_utf8(c2.stdout).unwrap().trim().to_string();

    let repo = Repo::open(root).expect("open");
    assert_eq!(repo.blob_at(&c1_sha, "f.rs").unwrap(), Some("v1\n".to_string()));
    assert_eq!(
        repo.blob_at(&c2_sha, "f.rs").unwrap(),
        Some("v1\nv2\n".to_string()),
    );
    // Missing path at a given commit is None, not an error.
    assert_eq!(repo.blob_at(&c1_sha, "ghost.rs").unwrap(), None);
}

#[test]
fn blob_sha_at_and_blob_by_sha_round_trip() {
    use std::fs;
    let td = tempdir().expect("tempdir");
    let root = td.path();
    let _ = gix::init(root).expect("init");

    fs::write(root.join("f.rs"), "some content\n").unwrap();
    use std::process::Command;
    Command::new("git")
        .args(["add", "-A"])
        .current_dir(root)
        .status()
        .unwrap();
    commit(root, "init");
    let out = Command::new("git")
        .args(["rev-parse", "HEAD"])
        .current_dir(root)
        .output()
        .unwrap();
    let head = String::from_utf8(out.stdout).unwrap().trim().to_string();

    let repo = Repo::open(root).expect("open");

    let sha = repo
        .blob_sha_at(&head, "f.rs")
        .expect("sha lookup")
        .expect("path exists");
    assert_eq!(sha.len(), 40);

    let content = repo
        .blob_by_sha(&sha)
        .expect("content fetch")
        .expect("blob decodable");
    assert_eq!(content, "some content\n");

    // Missing path → None, not error.
    assert_eq!(repo.blob_sha_at(&head, "ghost.rs").unwrap(), None);
    // Non-existent SHA → error.
    assert!(repo.blob_by_sha("0000000000000000000000000000000000000000").is_err());
}

#[test]
fn walks_history_newest_first() {
    let td = tempdir().expect("tempdir");
    let _ = gix::init(td.path()).expect("gix init");

    commit(td.path(), "first");
    commit(td.path(), "second");
    commit(td.path(), "third");

    let repo = Repo::open(td.path()).expect("open");
    let subjects: Vec<String> = repo
        .walk()
        .expect("walk")
        .map(|r| r.expect("commit meta").subject)
        .collect();

    assert_eq!(subjects, vec!["third", "second", "first"]);

    // First commit in the walk is HEAD; its parents point to the previous.
    let all: Vec<_> = repo
        .walk()
        .expect("walk")
        .map(|r| r.expect("commit meta"))
        .collect();
    assert_eq!(all.len(), 3);
    assert_eq!(all[0].parents.len(), 1, "HEAD has one parent");
    assert_eq!(all[2].parents.len(), 0, "root commit has no parents");
    assert_eq!(
        all[0].parents[0], all[1].sha,
        "HEAD's parent is the middle commit",
    );
}
