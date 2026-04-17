//! End-to-end `entropyx schema`:
//!   - command exits 0
//!   - stdout is valid JSON
//!   - the emitted schema describes the real `Summary` produced by a
//!     mini `scan` against a throwaway repo — a guardrail against the
//!     schema drifting out of lock-step with the serialized shape.
//!
//! We don't run a full JSON Schema validator (no v0.1 dep for that);
//! we just check structural invariants the schema asserts: every
//! required top-level key is present, FileRow.values has the declared
//! length, Event tags are in the declared enum.

use std::collections::HashSet;
use std::fs;
use std::path::Path;
use std::process::Command;
use tempfile::tempdir;

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

fn commit(cwd: &Path, time: i64, subject: &str) {
    run_git(cwd, &["add", "-A"]);
    let status = Command::new("git")
        .args([
            "-c",
            "user.name=T",
            "-c",
            "user.email=t@example.com",
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
fn schema_command_emits_valid_json_with_expected_shape() {
    let td = tempdir().expect("tempdir");
    let out = cli_cmd(td.path())
        .arg("schema")
        .output()
        .expect("spawn entropyx schema");
    assert!(
        out.status.success(),
        "exit non-zero; stderr={}",
        String::from_utf8_lossy(&out.stderr),
    );

    let v: serde_json::Value = serde_json::from_slice(&out.stdout).expect("stdout is valid JSON");

    assert_eq!(v["$schema"], "https://json-schema.org/draft/2020-12/schema",);
    assert!(
        v["$id"]
            .as_str()
            .unwrap()
            .starts_with("https://entropyx.dev/schema/tq1-")
    );
    assert_eq!(v["title"], "tq1 Summary");

    let defs = v["$defs"].as_object().expect("$defs is object");
    for key in [
        "Schema",
        "Dict",
        "FileRow",
        "Event",
        "EventHotspot",
        "EventOwnershipSplit",
        "EventApiDrift",
        "EventRename",
        "EventIncidentAftershock",
        "Handle",
        "Enrichments",
        "PullRequestRef",
        "SignalClass",
    ] {
        assert!(defs.contains_key(key), "missing $def: {key}");
    }
}

#[test]
fn emitted_schema_matches_scan_output_structure() {
    // Scan a tiny repo, then assert the resulting Summary fits the
    // shape the schema claims — every top-level key present, FileRow
    // width matches, every event's `kind` is one of the declared tags.
    let td = tempdir().expect("tempdir");
    let repo = td.path().join("repo");
    fs::create_dir(&repo).unwrap();

    run_git(&repo, &["init", "-q", "-b", "main"]);
    fs::write(repo.join("lib.rs"), "pub fn a() {}\n").unwrap();
    commit(&repo, 1_700_000_000, "seed");
    fs::write(repo.join("lib.rs"), "pub fn a() {}\npub fn b() {}\n").unwrap();
    commit(&repo, 1_700_010_000, "add b");

    let scan = cli_cmd(td.path())
        .arg("scan")
        .arg(&repo)
        .output()
        .expect("spawn scan");
    assert!(
        scan.status.success(),
        "scan failed: {}",
        String::from_utf8_lossy(&scan.stderr)
    );
    let summary: serde_json::Value = serde_json::from_slice(&scan.stdout).expect("summary JSON");

    let schema_out = cli_cmd(td.path()).arg("schema").output().expect("schema");
    let schema: serde_json::Value =
        serde_json::from_slice(&schema_out.stdout).expect("schema JSON");

    // Top-level required keys from the schema all exist in the summary.
    let required: Vec<&str> = schema["required"]
        .as_array()
        .unwrap()
        .iter()
        .map(|v| v.as_str().unwrap())
        .collect();
    for key in required {
        assert!(
            summary.get(key).is_some(),
            "summary missing `{key}` (declared required in schema)"
        );
    }

    // FileRow.values length from the schema matches every row.
    let declared_len = schema["$defs"]["FileRow"]["properties"]["values"]["minItems"]
        .as_u64()
        .unwrap() as usize;
    for row in summary["files"].as_array().unwrap() {
        let actual = row["values"].as_array().unwrap().len();
        assert_eq!(actual, declared_len, "FileRow.values width mismatch");
    }

    // Every event's `kind` tag must be one of the `const` values from
    // the schema's Event oneOf variants.
    let event_kinds: HashSet<String> = schema["$defs"]["Event"]["oneOf"]
        .as_array()
        .unwrap()
        .iter()
        .map(|variant| {
            let def_ref = variant["$ref"].as_str().unwrap();
            let def_name = def_ref.trim_start_matches("#/$defs/");
            schema["$defs"][def_name]["properties"]["kind"]["const"]
                .as_str()
                .unwrap()
                .to_string()
        })
        .collect();

    for event in summary["events"].as_array().unwrap() {
        let k = event["kind"].as_str().unwrap();
        assert!(
            event_kinds.contains(k),
            "summary emitted event kind `{k}` not in schema's declared set {event_kinds:?}",
        );
    }
}
