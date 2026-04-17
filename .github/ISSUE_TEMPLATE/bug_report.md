---
name: Bug report
about: entropyx gave a wrong answer, crashed, or misbehaved
title: "[bug] "
labels: bug
---

**What you ran**

```
entropyx <command> <args>
```

**What you expected**

A sentence or two. Include the output you thought you'd get, if relevant.

**What actually happened**

Paste stderr, stdout (trimmed), and the exit code.

**Reproduction**

- Repository (public URL if possible, or a minimal fixture)
- Commit SHA you were at
- OS / arch
- entropyx version: `entropyx --version`
- Rust toolchain: `rustc --version`

**Determinism check**

If the bug is a wrong number (not a crash), please run the scan twice
and confirm you got the same output both times. If you didn't, that's
itself a critical bug — lead with that.

```
entropyx scan <repo> | sha256sum
entropyx scan <repo> | sha256sum
```

**Additional context**

Any CLAUDE.md / RFC references you think are relevant. Any hypothesis
about what's going wrong. Speculation is welcome — just label it.
