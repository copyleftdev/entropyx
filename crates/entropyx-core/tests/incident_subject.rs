//! RFC-008 incident-subject detection: conventional-commits heuristic.

use entropyx_core::metric::is_incident_subject;

#[test]
fn canonical_conventional_commits_match() {
    assert!(is_incident_subject("fix: memory leak"));
    assert!(is_incident_subject("fix(auth): expired token"));
    assert!(is_incident_subject("fix!: breaking hotfix"));
    assert!(is_incident_subject("hotfix: prod down"));
    assert!(is_incident_subject("revert \"feat: broken change\""));
}

#[test]
fn space_separated_also_matches() {
    assert!(is_incident_subject("fix a crash"));
    assert!(is_incident_subject("hotfix build"));
    assert!(is_incident_subject("revert last commit"));
}

#[test]
fn case_insensitive() {
    assert!(is_incident_subject("FIX: uppercase"));
    assert!(is_incident_subject("HotFix: mixed"));
    assert!(is_incident_subject("Revert: capitalized"));
}

#[test]
fn leading_whitespace_trimmed() {
    assert!(is_incident_subject("  fix: indented"));
    assert!(is_incident_subject("\tfix: tabbed"));
}

#[test]
fn fixup_is_not_an_incident() {
    // `git commit --fixup` produces "fixup! <target>" — autosquash, not
    // incident response.
    assert!(!is_incident_subject("fixup! earlier commit"));
    assert!(!is_incident_subject("fixups for review"));
}

#[test]
fn prefix_and_similar_words_do_not_match() {
    assert!(!is_incident_subject("prefix: unrelated"));
    assert!(!is_incident_subject("affix implementation"));
    assert!(!is_incident_subject("refactor: extract helper"));
    assert!(!is_incident_subject("revertable change flag"));
    // This last one is borderline — starts with "revert" but means something
    // else. For v0.1 we accept the false positive; the rule is a heuristic.
}

#[test]
fn empty_or_benign_never_matches() {
    assert!(!is_incident_subject(""));
    assert!(!is_incident_subject("   "));
    assert!(!is_incident_subject("feat: add widget"));
    assert!(!is_incident_subject("chore: bump deps"));
    assert!(!is_incident_subject("docs: tweak readme"));
}
