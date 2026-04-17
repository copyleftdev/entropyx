<!--
Thanks for the PR. Please keep the description tight — the
instrument matters more than the ceremony.
-->

## What changed

One or two sentences. What the code now does that it didn't before.

## Why

The question the user (human or AI) was trying to answer. If this
PR was motivated by a real scan, paste the relevant snippet.

## Contract impact

- [ ] No change to the tq1 `Summary` envelope
- [ ] Backwards-compatible additive change (new optional field, new
      event variant, etc.)
- [ ] Breaking change — `CONTRACT_VERSION` bumped and
      `crates/entropyx-tq/src/schema.rs` updated in this PR
- [ ] CLI output format changed (describe + usage updated)

## Determinism

- [ ] No new `SystemTime::now()` / wall-clock reads
- [ ] All new f64 reductions go through `metric::reduce_sum`
- [ ] Output is byte-identical across two successive runs on the
      same input (verified with `sha256sum`)

## Tests

- [ ] New tests land with this PR
- [ ] `cargo test --workspace` passes
- [ ] `cargo clippy --workspace --all-targets` clean
- [ ] If this touches a language backend, real-world scan on an OSS
      repo of that language produces sane output

## Credit

If you want a line in `AUTHORS`, add it here in the same PR.
