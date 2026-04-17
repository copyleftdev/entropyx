---
name: Feature request
about: Propose a new measurement, signal class, language backend, or CLI capability
title: "[feat] "
labels: enhancement
---

**The question you're trying to answer**

What do you (or Claude, or your team) need entropyx to tell you that
it currently can't? Describe the user standing in front of the output.

**Why the current tool can't answer it**

Link the axis / signal / command you tried and what it returned.

**Proposed shape**

If it's a new metric: what would it measure, what would the range be,
how would it be bounded in `[0, 1]`?

If it's a new signal class: what's the rule, and what existing axes
does it combine?

If it's a new language: the file extensions and the visibility
convention we'd need to capture (private-by-default? keyword-based?).

If it's a CLI change: what's the invocation, what's the output shape,
and does it fit the tq1 contract?

**Determinism and local-first impact**

Does this require network, ML, or wall-clock time? If yes, it goes
behind an opt-in flag at best.

**Alternatives considered**

Is there a way to answer this question by post-processing existing
output instead? (Sometimes there is. We'd rather ship a small
documented recipe than a new command.)
