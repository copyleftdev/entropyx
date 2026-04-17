//! End-to-end: build a real git fixture with known authors and commit
//! cadence, walk it through entropyx-git, and feed the author/time bags
//! into entropyx-core's physics primitives. Proves the collector →
//! physics pipeline is wired correctly.

use entropyx_core::metric::{author_dispersion, temporal_volatility};
use entropyx_git::Repo;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

fn commit_as(cwd: &Path, name: &str, email: &str, time: i64, subject: &str) {
    let status = Command::new("git")
        .args([
            "-c",
            &format!("user.name={name}"),
            "-c",
            &format!("user.email={email}"),
            "commit",
            "--allow-empty",
            "-q",
            "-m",
            subject,
        ])
        .env("GIT_AUTHOR_DATE", format!("@{time} +0000"))
        .env("GIT_COMMITTER_DATE", format!("@{time} +0000"))
        .current_dir(cwd)
        .status()
        .expect("spawn git");
    assert!(status.success(), "git commit failed for {subject}");
}

#[test]
fn collector_feeds_physics_primitives() {
    let td = tempdir().expect("tempdir");
    let _ = gix::init(td.path()).expect("gix init");

    // 5 commits: alice ×3, bob ×2. Exponentially-growing gaps: 100, 200,
    // 400, 800. Deliberately non-uniform so both metrics produce
    // non-trivial values.
    commit_as(td.path(), "Alice", "alice@ex.com", 100, "a1");
    commit_as(td.path(), "Bob", "bob@ex.com", 200, "b1");
    commit_as(td.path(), "Alice", "alice@ex.com", 400, "a2");
    commit_as(td.path(), "Bob", "bob@ex.com", 800, "b2");
    commit_as(td.path(), "Alice", "alice@ex.com", 1600, "a3");

    let repo = Repo::open(td.path()).expect("open");
    let metas: Vec<_> = repo
        .walk()
        .expect("walk")
        .map(|r| r.expect("meta"))
        .collect();
    assert_eq!(metas.len(), 5, "all 5 commits walked");

    // Author dispersion: 2 distinct authors, skewed 3:2 → below 1.
    let emails: Vec<&str> = metas.iter().map(|m| m.author.email.as_str()).collect();
    let d = author_dispersion(&emails);
    assert!(d > 0.9 && d < 1.0, "dispersion {d} not in (0.9, 1.0)");

    // Temporal volatility: gaps 100/200/400/800, CV > 0.5.
    let times: Vec<i64> = metas.iter().map(|m| m.committer.time).collect();
    let v = temporal_volatility(&times);
    assert!(v > 0.5, "volatility {v} lower than expected");

    // Determinism: repeat the computation, confirm bitwise-stable.
    let d2 = author_dispersion(&emails);
    let v2 = temporal_volatility(&times);
    assert_eq!(d.to_bits(), d2.to_bits(), "RFC-001 bitwise stability (H_a)");
    assert_eq!(v.to_bits(), v2.to_bits(), "RFC-001 bitwise stability (V_t)");
}
