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

#[test]
fn go_underscore_test_suffix() {
    assert!(is_test_path("internal/config/config_test.go"));
    assert!(is_test_path("pkg/foo/bar_test.go"));
    assert!(!is_test_path("pkg/foo/bar.go"));
}

#[test]
fn js_and_ts_dot_test_and_spec_suffixes() {
    for f in [
        "src/foo.test.js", "src/foo.test.ts",
        "src/Bar.test.tsx", "src/x.test.jsx",
        "src/y.spec.ts", "src/z.spec.js",
        "src/m.test.mjs", "src/c.test.cjs",
    ] {
        assert!(is_test_path(f), "{f} should be a test path");
    }
    assert!(!is_test_path("src/foo.js"));
    assert!(!is_test_path("src/attest.js"));
}

#[test]
fn python_conventions_prefix_and_suffix() {
    assert!(is_test_path("tests/test_widget.py"));
    assert!(is_test_path("project/test_module.py"));
    assert!(is_test_path("project/widget_test.py"));
    assert!(is_test_path("tests.py"));
    assert!(!is_test_path("project/testify.py"));
}

#[test]
fn ruby_spec_and_test_suffixes() {
    assert!(is_test_path("spec/models/user_spec.rb"));
    assert!(is_test_path("test/user_test.rb"));
    assert!(is_test_path("app/user_spec.rb"));
    assert!(!is_test_path("app/user.rb"));
}

#[test]
fn java_test_classes() {
    assert!(is_test_path("src/test/java/FooTest.java"));
    assert!(is_test_path("src/WidgetTests.java"));
    assert!(is_test_path("src/ServiceSpec.java"));
    assert!(!is_test_path("src/Service.java"));
}

#[test]
fn cpp_test_suffixes() {
    assert!(is_test_path("src/widget_test.cc"));
    assert!(is_test_path("src/widget_test.cpp"));
    assert!(is_test_path("src/widget_tests.cxx"));
    assert!(!is_test_path("src/widget.cpp"));
}

#[test]
fn js_test_directory_conventions() {
    assert!(is_test_path("__tests__/foo.js"));
    assert!(is_test_path("src/__tests__/bar.ts"));
    assert!(is_test_path("test/helper.js"));
}
