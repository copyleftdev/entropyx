# Contributing to entropyx

Thanks for considering a contribution. A few notes on how this project thinks
about changes — read these before opening a non-trivial PR and it'll save
both of us time.

## License

entropyx is licensed under the **GNU Affero General Public License
version 3 or later (AGPL-3.0-or-later)**. By submitting a contribution
you agree that your work will be licensed the same way, and that your
commit's `Co-Authored-By:` (or PR authorship) is the record of your
attribution.

**Read this before contributing:** AGPL-3.0 is a strong copyleft
license with a network clause. If you or your employer ship a service
that lets users interact with a modified entropyx over a network, you
must make the modified source available to those users. If that's
incompatible with your situation, talk to the maintainer before
opening a PR — a separately-licensed downstream version can be
discussed.

If you want explicit credit in `AUTHORS`, add yourself in your first PR.

## What we're optimizing for

entropyx is a **forensic instrument**. The bar isn't "does this add a
feature" — it's "does this give an honest, reproducible, fast answer
to a question an SRE or engineering lead is asking at 3am."

That implies a few opinionated rules:

1. **Determinism is load-bearing.** No ML models. No wall-clock reads.
   No platform-dependent output. Every sum goes through
   `entropyx_core::metric::reduce_sum` so f64 ordering is stable
   (RFC-001). If your PR would break bitwise reproducibility, it needs
   a very compelling story.

2. **Local-first, no network in the core path.** `entropyx scan` must
   work on a disconnected laptop. Network is allowed *only* behind
   opt-in flags like `--github`, and those paths must degrade
   gracefully when the network is gone or rate-limited.

3. **Typed protocol, versioned.** Anything user-visible in the tq1
   `Summary` envelope is part of the contract in `entropyx-tq`.
   Breaking changes require bumping `CONTRACT_VERSION`
   (`entropyx-core`) and updating `crates/entropyx-tq/src/schema.rs`
   in the same PR. The JSON Schema's `$id` must move in lock-step.

4. **Honest emptiness.** When the tool doesn't know, it returns zero
   (or `None`, or an empty vec) and says so. Don't add confident lies
   to fill a gap — Claude is really good at believing those.

## Design axes before writing code

Before implementing anything non-trivial, think about where your change
lands:

- **Measurement (new axis, new classifier, new event)?** Read
  `CLAUDE.md` for the RFC-series that governs that surface. RFC-007
  covers composite scoring; RFC-008 covers classifier rules; RFC-012
  covers calibration.
- **New language backend?** Add an `entropyx-ast/src/<lang>_lang.rs`
  that implements `parse_public_items`, register it in
  `Language` and `language_from_path`, and add tests that assert
  visibility filtering (private members, access specifiers, etc.)
  works on realistic fixtures.
- **CLI change?** Keep commands boring and composable. stdin, stdout,
  exit codes, JSON. No TUI, no interactive prompts. Update
  `print_usage()` and `entropyx describe` in the same commit.

## Running the tests

```bash
cargo test --workspace                    # full suite (currently 289 tests)
cargo test -p entropyx-ast ruby_lang      # focused
cargo clippy --workspace --all-targets    # lints must be clean
```

For end-to-end validation, run the binary against a real repo of your
choice. The `README.md` "How we know it works" section lists the repos
we've already dogfooded on — feel free to add one.

## Commit / PR style

- Prefer many small commits over one big one.
- Commit subjects should be imperative and scoped: `fix: ...`,
  `feat(ast): ...`, `refactor(tq): ...`. The `fix:` / `hotfix:`
  prefixes are load-bearing — they're what entropyx's own
  `IncidentAftershock` detector matches against.
- PR descriptions should explain the *why* (what was the question
  the user was trying to answer?) before the *what*. If the change
  was motivated by a real repo entropyx stumbled on, say so and
  attach the scan output snippet.

## Reporting issues

See `SECURITY.md` for security-sensitive reports. For everything else,
open a GitHub issue. If you can attach the `entropyx describe` output,
the command that failed, and (when safe) a trimmed repo that
reproduces, you'll get a response much faster.

## Why the bar is what it is

This is a tool that other people will rely on to answer production
questions. A regression here means someone reads the wrong number at
3am and makes the wrong call. That's why the tests are assertive,
the determinism is bitwise, and the contract is typed. The annoyance
is the point.
