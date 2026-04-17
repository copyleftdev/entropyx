# Changelog

All notable changes to entropyx will be recorded here. Format:
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/).
Versioning: [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

The tq1 protocol's `CONTRACT_VERSION` tracks the `[workspace.package]`
version and is pinned into the JSON Schema's `$id`. A breaking tq1
change requires a major version bump.

## [Unreleased]

### Added

- `entropyx schema` — emits the tq1 `Summary` JSON Schema (draft
  2020-12) with `$id` pinned to `CONTRACT_VERSION`.
- Test-file detection for Go (`_test.go`), JS/TS (`.test.*`,
  `.spec.*`, `__tests__/`), Python (`test_*.py`, `*_test.py`),
  Ruby (`_spec.rb`, `_test.rb`), Java (`*Test.java`,
  `*Tests.java`, `*Spec.java`), and C++ (`_test.cc` and kin).

### Changed

- Ruby parser (`entropyx-ast`) now tracks `private`/`protected`
  sections and `private :name` / `private_class_method :name`
  calls; private methods are excluded from public-API captures.
- Python parser scopes methods by enclosing class path
  (`method:Class.method`) so two classes with identical method
  names produce distinct signatures.
- C++ parser respects `class` (default private) vs `struct`/`union`
  (default public) defaults and flips on `public:`/`private:`/
  `protected:` access specifiers within record bodies.
- JavaScript parser captures named and anonymous function
  expressions on `module.exports` (`cjs:name` / `cjs:default`).
- Java parser excludes Java 9+ explicit `private` methods inside
  interfaces.

### Fixed

- `diff_from_parent` at a shallow-clone boundary no longer hard-
  fails on a missing parent object; the commit is treated as a
  root, matching git's shallow-boundary semantics.

## [0.1.0] — initial public release

First complete, deterministic, local-first version. All seven
metric axes live, all six signal classes, all five event variants,
seven language backends (Rust, Go, Python, TypeScript, Java,
JavaScript, Ruby, C++), five CLI commands (`describe`, `scan`,
`explain`, `calibrate`, `schema`).

Validated by dogfooding on ripgrep, rich (Python, 5.8 years),
Jekyll (Ruby), re2 (C++), RoomIQ (Go/TS), and entropyx itself.
See the README's "How we know it works" section for findings.

Determinism invariants (RFC-001): all f64 reductions go through
`metric::reduce_sum`; no wall-clock reads; interning stable across
serde round-trips.
