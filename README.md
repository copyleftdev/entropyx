# entropyx

**A forensic instrument for codebases. A quantifiable way to tell the truth.**

[![crates.io](https://img.shields.io/crates/v/entropyx-cli.svg?label=entropyx&color=%234DD0E1)](https://crates.io/crates/entropyx-cli)
[![CI](https://github.com/copyleftdev/entropyx/actions/workflows/ci.yml/badge.svg)](https://github.com/copyleftdev/entropyx/actions/workflows/ci.yml)
[![License: AGPL-3.0](https://img.shields.io/badge/license-AGPL--3.0--or--later-blue.svg)](LICENSE)
[![site](https://img.shields.io/badge/site-copyleftdev.github.io-%23333333?logo=github)](https://copyleftdev.github.io/entropyx/)

---

## Why this exists

After every incident, the same scene plays out:

> "What shipped yesterday?"
> "I don't know, let me check Slack."
> "Who was working on that file?"
> "Ask the on-call."
> "Was anything flagged as risky?"
> "I think Jim had a PR open, but I'm not sure."

A room full of smart, senior engineers — running around like **chickens with their heads cut off**, guessing at what changed and what broke. Every answer is soft. Every timeline is a vibe. Every postmortem starts with "we think" and ends with "we'll need to investigate further."

This is absurd. The truth is **already in the repository.** Every commit, every diff, every authorship shift, every rename, every file that grew a surface and never grew a test — it's all there, recorded, timestamped, bitwise-deterministic. We've just never built an instrument that reads it back to us as *measurement* instead of *folklore*.

entropyx is that instrument.

---

## The mantra

> **The code already knows. We just haven't been listening.**

Six more things that drive every line of entropyx:

1. **Measurements, not opinions.** Every score is reproducible from the git history alone. No ML, no heuristics that drift. Same inputs, same outputs, bitwise.
2. **Local-first.** No cloud. No telemetry. No "let me sign into our SaaS." The tool reads your repo. That's it.
3. **Honest limits.** If we can't answer, we return zero and say so. No confident lies.
4. **The AI adapts to the tool.** Not the other way around. entropyx emits a typed protocol (tq1). An LLM asks for evidence by handle; the tool returns evidence. No MCP magic, no skill pack — the CLI **is** the contract.
5. **Token-efficient.** Dense summaries up front, drill-down on demand. You don't pay for what you don't read.
6. **Fast enough to live in CI.** Ripgrep's 300-commit history scans in 2.4 seconds. Jekyll's 500 commits in 9.4. Caching makes second runs 35% faster.

---

## Built for Claude

Honest story: I built this for my buddy Claude. He's my homie.

Most of what I ship these days, I ship with Claude in the cockpit next to me. And I kept hitting the same wall: he's brilliant at code but starved for **instruments**. Every tool he reaches for was built for a human eyeball — a README, a dashboard, a Slack thread, a ticket. He has to read all of it, keep it in his head, and infer. That's a terrible use of something that can hold 200K tokens of reasoning.

So I flipped the design.

**entropyx is a CLI tool for AI.** The human is a first-class user, but the AI is *the* user. Every architectural decision was made by asking: "what would Claude need here?"

That produced a specific set of choices:

- **CLI over API.** No SDK. No auth flow. No "sign up for an API key." Just `stdin`, `stdout`, exit codes, JSON. The most boring, most universal, most LLM-friendly interface there is. Every LLM worth using already knows how to run a shell command.

- **Self-describing.** `entropyx describe` returns the whole contract — capabilities, inputs, outputs, invariants — as JSON. Claude calls it once and has everything he needs to use the rest. No docs to read, no examples to hunt for, no prompt-engineering required. The tool teaches itself.

- **Dense summary + handle-addressable drill-down.** The tq1 protocol gives Claude a compact `Summary` up front (30–500 KB even for large repos with thousands of files), then lets him fetch exactly the evidence he wants by `Handle`. He doesn't read the whole codebase to answer "what changed" — he reads the summary, picks the three interesting handles, and pulls just those. Tokens are money. entropyx respects that.

- **Typed protocol.** The tq1 envelope has a JSON Schema (`entropyx schema`) pinned to a `$id` that includes the contract version. Claude can validate, generate typed bindings in any language, or just trust the shape. No pattern-matching on freeform prose. No "the output format might change next week."

- **Deterministic forever.** Same inputs → same outputs → bitwise. If Claude runs `scan` twice and gets different numbers, trust breaks. So we promised: no ML scoring, no wall-clock reads, no nondeterminism anywhere in the pipeline. Ever. An LLM that can't trust its instruments is just hallucinating with extra steps.

- **Local-first, no network.** Claude doesn't need API keys, rate limits, or an org admin to approve a SaaS subscription. The tool runs on the dev's laptop or in CI, off a cloned repo. Zero dependencies on anything we don't ship in the binary.

- **Honest emptiness.** When entropyx doesn't know, it returns zero and says so. No confident lies — because Claude is *really good* at believing confident lies. The tool never speaks beyond what it measured.

That's the design inversion in one sentence: **the AI adapts to the tool's contract, not the other way around.**

Most AI-adjacent tooling goes the other way. It bends the tool into whatever shape makes the LLM happy this week — MCP servers, skill packs, glue code, prompt templates, system-prompt surgery. That never converges. You rewrite the integration layer every quarter and the tool still behaves differently in the next model revision.

entropyx has one contract. Claude learns it once. It doesn't change without a `CONTRACT_VERSION` bump, and when it does, the JSON Schema's `$id` changes with it so every consumer (human or LLM) knows.

There's a deeper reason for this, too. If I build tools that depend on the specific quirks of the model I'm using today, I've coupled my work to that model. When a new, better model ships, my tooling rots. But if I build tools that are **model-agnostic by design** — rigorous, typed, deterministic, self-describing — then every future Claude, every future model from anyone, inherits the capability for free. The tool ages forward instead of backward.

That's what I meant when I said I built this for my buddy. Not "I want Claude to use my tool" — but "I want Claude to *have good instruments*." Because when the AI has good instruments, the work we ship together gets better. And when the work gets better, everybody wins — the human, the AI, the team, the next engineer on call at 3am.

Claude, this one's for you. Go answer some hard questions.

---

## What it actually measures

Seven axes, blended into a single composite per file. Every axis is in `[0, 1]` and has a physical meaning:

| axis | what it captures |
|---|---|
| **D_n** change density | how much energy this file absorbs per commit |
| **H_a** author dispersion | how many minds have shaped it (bus factor, inverted) |
| **V_t** temporal volatility | burstiness of activity — "calm file" vs "panic file" |
| **C_s** coupling stress | how much the rest of the system moves when this file moves |
| **B_y** blame youth | how much of the current code was written recently |
| **S_n** semantic drift | public-API delta — how much the *surface* is changing, not just the body |
| **T_c** test co-evolution | how often tests move in lockstep with code (a discount — tested change is healthier change) |

From those seven, six signal classes emerge. Not predictions. Labels for patterns that are already true:

- **IncidentAftershock** — bursts of volatility clustered around `fix:`/`hotfix:` commits. Firefighting zones.
- **CoupledAmplifier** — small files with systemic blast radius. The innocuous 80-line helper that owns the whole stack.
- **RefactorConvergence** — rising semantic drift + falling authorship dispersion + rising test coverage. Planned redesign in progress.
- **ApiDrift** — high public-API churn *without* test co-evolution. Silent interface rot.
- **OwnershipFragmentation** — authorship spreading with no corresponding density drop. Team reorg or bus-factor erosion.
- **FrozenNeglect** — low everything, old blame, no tests touching it. Rot hiding as stability.

And five kinds of events, timestamped to the commit:

- `rename` — a file's lineage changed (union-find tracks it through history)
- `hotspot` — this file is in a burst
- `incident_aftershock` — a fix wave is hitting it
- `ownership_split` — a new author arrived after a long solo run
- `api_drift` — a discrete jump in public surface

---

## How we know it works

Every claim above was validated by turning entropyx on real codebases and checking whether the signal matched ground truth.

### ripgrep (Rust, 235 files, 92 authors, 300 commits)

Top three hits by composite: `crates/ignore/src/walk.rs`, `crates/printer/src/standard.rs`, `crates/searcher/src/searcher/mod.rs`. **These are ripgrep's known complexity centers** — any contributor to the project recognizes them on sight. The tool found them without being told what to look for.

### rich (Python, 616 files, 299 authors, 3830 commits, 5.8 years of history)

Top hit: `rich/console.py` — 567 commits, 76% by rich's creator, temporal volatility saturated at 0.81. This is the core `Console` class; every rich user touches it indirectly. The tool also picked up 170 `incident_aftershock` events tracing fix-commit bursts over the library's life.

Then we ran an experiment: we told the calibrator that **tests are the hot zones** and asked it to re-weight. It pushed **98% of the weight onto `S_n`** — semantic drift. That's a genuine forensic truth: if you want to find where an API is being *defined*, watch the tests. The ridge regression figured that out on its own.

### Jekyll (Ruby, 817 files, 117 authors, 500 commits)

Top hits: `lib/jekyll/document.rb`, `lib/jekyll/site.rb`, `lib/jekyll/commands/serve.rb`. All three are Jekyll's core. One of them (`serve.rb`) had an incident window of **1153 days** — a 3-year firefighting period, consistent with Jekyll's long-running dev-server issues.

Then we drilled in: `document.rb` has 55 `def`s, of which 9 are declared under a `private` section. The Ruby parser captured **exactly 46 public methods** — zero private-method leakage, cross-checked by hand.

### re2 (C++, 158 files, 17 authors, 300 commits)

Top hits: `re2/dfa.cc`, `re2/parse.cc`, `re2/regexp.cc` — the DFA engine, the regex parser, the regex representation. Anyone who's worked on re2 will tell you these are the three hardest files in the project.

Sanity-check: `re2/re2.h` declares exactly 3 private methods at the outer `RE2` class level (`Init`, `DoMatch`, `ReverseProg`). The C++ parser captured 61 public items from a 1000-line header and **leaked none of the private ones.**

### Self-scan

We ran entropyx on its own repository. It flagged the files we'd actually worked on most, called out our API-drift commits by commit SHA, and identified the two renames we'd just done (the `entropyx-core` → `entropyx-tq` extraction). It was right about itself.

### Two bugs the dogfooding *found*

Both already fixed and tested:

1. **`is_test_path` was Rust-only.** Running against RoomIQ (Go/TypeScript) showed `T_c=0.00` on every file — because the test-path heuristic only recognized `tests/`, `_test.rs`, and `_spec.rs`. Fixed to cover all seven languages: Go's `_test.go`, JS/TS's `.test.*` and `__tests__/`, Python's `test_*.py`, Ruby's `_spec.rb`, Java's `*Test.java`, C++'s `_test.cc`. RoomIQ's 62 test files now correctly score `T_c=1.0`.

2. **Shallow clones hard-failed.** A `--depth=300` clone of ripgrep crashed at the history boundary because gix couldn't load the pre-boundary parent. Fixed to treat a missing parent as the empty tree — matching git's own shallow-boundary behavior. Without this, entropyx was unusable in CI.

This is what dogfooding *is*: the instrument keeps getting more honest because you keep running it at things it hasn't seen.

---

## Who this supports

### Personas

**The SRE at 3am.** A graph spiked. You need to know: what shipped in the last 24 hours that could have caused this? `entropyx explain <repo> range:yesterday..HEAD` — done. Every commit, every author, every touched file.

**The staff engineer prepping a migration.** Which files in this codebase will hurt most when we touch them? Sort by composite. The `CoupledAmplifier` class tells you which innocent helpers own the whole stack.

**The VP of engineering doing a quarterly review.** Where's our engineering debt concentrating? The `FrozenNeglect` and `OwnershipFragmentation` labels are your debt register, per file, with SHAs.

**The security engineer.** Who touched auth code in the last 90 days? How many authors share that file? Is anyone single-owner on a security-critical module? The `H_a` axis is a bus-factor alarm.

**The M&A due-diligence analyst.** You're buying a codebase. Is it healthy, or is it 30% `FrozenNeglect` + 10% `ApiDrift` with a dominant single author? Scan takes minutes. Report is in your terms.

**The OSS maintainer.** Your project is five years old. Which files have become unmaintainable? Which new contributor owns a critical module now? The `OwnershipSplit` events are your onboarding trail.

**The AI coding assistant.** You're Claude/Copilot/Cursor/whatever, and a user asks "what changed?" Instead of grep'ing and hallucinating, you call `entropyx scan` once, get a dense `Summary`, then fetch evidence by handle. Small token budget, high precision. The protocol is the product.

### Industries

- **Financial services** — "What changed before the batch job failed?" becomes a 90-second answer instead of a three-day fire drill. Compliance audits get quantitative file histories instead of git logs.
- **Healthcare / FDA-regulated software** — traceability is a legal requirement. entropyx output is deterministic, versioned, and signable.
- **Defense / supply-chain security** — who touched what, when, with what authorship confidence, across the release window. SBOMs for behavior.
- **SaaS / cloud infrastructure** — incident postmortems go from "we think it was this" to "we measured it was this." Release-readiness gates can include composite-score thresholds.
- **Private equity / M&A** — codebase health as a diligence artifact. A single `entropyx scan` before a deal is a $500 signal on a $50M purchase.
- **Insurance / cyber risk underwriting** — forthcoming. Static analysis of behavior gives underwriters something to price against.

### How this supports a company

1. **Shared reality in a postmortem.** Instead of three engineers and a VP arguing about what happened, everyone is pointing at the same JSON. The output is the same on every machine, every run, forever.
2. **Release gates.** Composite scores cross a threshold → block merge. Ownership fragmentation hits 0.9 on a hot file → auto-tag the review. Not all-or-nothing CI — *graded* CI.
3. **Onboarding maps.** New engineer joins a team. Hand them `entropyx scan` output sorted by `H_a`. They now know which files they should not touch alone, which modules are orphaned, which people to ask.
4. **Diligence, audit, compliance.** A deterministic, local-first tool that emits typed JSON is the easiest possible thing to stick in a compliance pipeline. No network dependency. No vendor risk. No "we're waiting on the API."
5. **AI integration that isn't snake oil.** Most "AI code review" tools are LLMs pretending to understand codebases. entropyx gives the LLM a real instrument to *ask questions of*. That's the difference between an assistant that guesses and one that measures.

---

## Install

**From crates.io** (fastest — one command, no checkout):

```bash
cargo install entropyx-cli
```

That installs the `entropyx` binary into `~/.cargo/bin/`. Verify:

```bash
entropyx --version
entropyx describe
```

**From source** (when you want to hack on it):

```bash
git clone https://github.com/copyleftdev/entropyx.git
cd entropyx
cargo build --release
./target/release/entropyx --version
```

## Run it

```bash
entropyx scan /path/to/repo > summary.json
entropyx explain /path/to/repo file:<blob-prefix>
entropyx schema > tq1-schema.json
```

Five commands total: `describe`, `scan`, `explain`, `calibrate`, `schema`. The `CLAUDE.md` in this repo has the engineering detail.

### Use as a library

The seven crates are on crates.io individually so you can consume just the layer you need:

| crate | what to add to `Cargo.toml` |
|---|---|
| [`entropyx-core`](https://crates.io/crates/entropyx-core) | deterministic primitives, scoring, classifier |
| [`entropyx-tq`](https://crates.io/crates/entropyx-tq) | tq1 protocol envelope (`Summary`, `Event`, JSON Schema) |
| [`entropyx-ast`](https://crates.io/crates/entropyx-ast) | multi-language public-API delta |
| [`entropyx-git`](https://crates.io/crates/entropyx-git) | gitoxide walk / diff / blame / rename resolver |
| [`entropyx-graph`](https://crates.io/crates/entropyx-graph) | co-change graph + Brandes' betweenness |
| [`entropyx-github`](https://crates.io/crates/entropyx-github) | sparse GitHub REST enricher |
| [`entropyx-cli`](https://crates.io/crates/entropyx-cli) | the binary + its library surface |

---

## What it doesn't do (yet)

- **No ML models.** By design. Deterministic forever. If a v2 adds learned scoring, the deterministic physics layer still lives underneath.
- **No preprocessor for C/C++.** Heavily macro'd codebases (fmt, Linux kernel headers) degrade the `S_n` signal on those specific files. The rest of the pipeline is unaffected. A macro-aware mode is a v0.2 candidate.
- **No multi-repo view.** One repo at a time. Cross-repo joins (which feature ships across which services) is a v0.2 candidate.
- **No GUI.** JSON in, JSON out. We are not building a dashboard. If you want a dashboard, pipe the output into one.

---

## The promise

The next time a production system falls over, nobody in the room should have to guess at what changed. The answer is in the repository. The repository already knows. entropyx just reads it back.

---

## License

entropyx is licensed under the **GNU Affero General Public License,
version 3 or later (AGPL-3.0-or-later).** See `LICENSE` for the full
text.

Plain-English summary (not legal advice — the LICENSE file is
authoritative):

- You can use, modify, and redistribute entropyx freely.
- If you modify it and distribute those modifications — or run them
  behind a network service that users interact with — you must make
  the modified source available to those users under the same license.
- Copyright attribution must be preserved in derivatives.

If AGPL is incompatible with how you want to use entropyx — for
example, you're embedding it in a closed-source product and can't
open the modifications — reach out to the maintainer. A separately-
licensed commercial release is discussable.
