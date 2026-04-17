//! Test-file path classification (v0.1 heuristic).

use entropyx_core::metric::is_test_path;

#[test]
fn tests_directory_segments() {
    assert!(is_test_path("tests/foo.rs"));
    assert!(is_test_path("crates/foo/tests/bar.rs"));
    assert!(is_test_path("deep/nested/tests/x.rs"));
}

#[test]
fn underscore_suffix_conventions() {
    assert!(is_test_path("src/foo_test.rs"));
    assert!(is_test_path("src/foo_tests.rs"));
    assert!(is_test_path("src/models_spec.rs"));
}

#[test]
fn tests_rs_module_file() {
    assert!(is_test_path("src/tests.rs"));
    assert!(is_test_path("tests.rs"));
}

#[test]
fn source_files_are_not_tests() {
    assert!(!is_test_path("src/main.rs"));
    assert!(!is_test_path("src/lib.rs"));
    assert!(!is_test_path("src/foo.rs"));
    assert!(!is_test_path("Cargo.toml"));
    assert!(!is_test_path(""));
}

#[test]
fn word_boundary_matters() {
    // "teststuff.rs" is not a test file — the suffix check needs an
    // underscore; the segment check needs the slash.
    assert!(!is_test_path("src/teststuff.rs"));
    assert!(!is_test_path("testsupport/foo.rs"));
}

#[test]
fn windows_style_separators_normalized() {
    assert!(is_test_path("crates\\foo\\tests\\bar.rs"));
    assert!(is_test_path("src\\foo_test.rs"));
}
