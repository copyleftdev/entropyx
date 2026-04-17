# entropyx

A Rust-native forensic instrument for codebase dynamics. Scans a local git
repository, projects its temporal/structural/authorship trajectory into a
dense tq1 `Summary`, and exposes handle-addressable drill-down for AI
consumers.

**Design inversion: AI adapts to the tool's contract, not the other way
around.** No MCP magic, no skill pack — the CLI itself is the contract.

---

## Crate layout

| crate | purpose |
|---|---|
| `entropyx-core` | types, determinism primitives, RFC-007 physics, RFC-008 classifier, RFC-012 calibration |
| `entropyx-git` | gitoxide-backed collector: walk, diff, blame, blob access, RFC-004 lineage resolver |
| `entropyx-ast` | public-API delta backends: Rust (syn), Go/Python/TypeScript (tree-sitter) |
| `entropyx-graph` | `CoChangeGraph` + Brandes' betweenness centrality |
| `entropyx-github` | sparse REST enricher via ureq (v0.1 exposes `pr_for_commit`) |
| `entropyx-cli` | four commands: `describe`, `scan`, `explain`, `calibrate` |
| `entropyx-tq` | not yet extracted — Summary/Dict/FileRow live in `entropyx-core::summary` |

Workspace-wide determinism invariant (RFC-001): all f64 reductions go
through `metric::reduce_sum`; no wall-clock reads; interning is stable
across serde round-trips.

---

## CLI commands

### `describe`

```bash
entropyx describe
```

Emits the self-identifying protocol root: capabilities, inputs/outputs,
invariants. This is how an AI consumer bootstraps — no external docs
required. Tests: `entropyx-core/tests/schema_roundtrip.rs`.

### `scan <repo-path> [--weights <path>]`

Walks HEAD newest-first, diffs each commit against its parent (empty tree
for root), aggregates per-file metrics, builds a CoChangeGraph, emits a
tq1 `Summary`:

```json
{
  "schema": {"name": "tq1", "version": "0.1.0"},
  "dict": {"files": [...], "authors": [...], "metrics": [...]},
  "files": [{"file": <FileId>, "values": [D, H, V, C, B, S, T, composite], ...}],
  "events": [{"kind": "rename"|"hotspot"|"incident_aftershock"|"ownership_split"|"api_drift", ...}],
  "handles": {"file:<blob-prefix>": {...}}
}
```

`--weights <path>` loads a `ScoreWeights` JSON (e.g. calibrated output)
and uses it instead of `DEFAULT_WEIGHTS`.

### `explain <repo-path> <handle | file-path> [--github [owner/name]]`

Dispatches on handle kind:

| input | output |
|---|---|
| `file:<blob-prefix>` | per-file evidence (resolved via HEAD tree blob match) |
| `<path>` (no prefix) | same shape as `file:` |
| `commit:<sha>` | commit metadata + change list + stats |
| `range:<base>..<head>` | commit set reachable from head but not base |

`--github` enriches `commit:` handles with the PR that introduced the
commit. Bare `--github` auto-detects `owner/name` from `origin`'s
remote URL; `--github owner/name` overrides explicitly.

### `calibrate --summary <path> --labels <path>`

RFC-012 weight fitting. Takes a prior `scan` output and a labels file
(JSON `{path: score_in_[0,1]}`), joins on path, runs ridge regression
via gradient descent, emits a `ScoreWeights` JSON. Use the output with
`scan --weights` for a calibrated re-scan.

Pipeline: `scan → summary.json → calibrate → weights.json → scan --weights`.

---

## The composite formula (RFC-007)

```
composite = 0.15·D_n + 0.15·H_a + 0.10·V_t + 0.20·C_s + 0.10·B_y + 0.30·S_n − 0.05·T_c
```

`DEFAULT_WEIGHTS` positives sum to 1.0 (RFC-012 invariant). Every input
is in `[0, 1]` so composite is bounded by `[−0.05, 1.0]`.

| axis | name | source | range |
|---|---|---|---|
| D_n | change density | `change_counts` → `unit_normalize` | [0, 1] |
| H_a | author dispersion | `author_dispersion(emails)` per file | [0, 1] |
| V_t | temporal volatility | `saturate_unit(temporal_volatility(times))` | [0, 1) |
| C_s | coupling stress | `max(normalized_degree, betweenness)` via CoChangeGraph | [0, 1] |
| B_y | blame youth | fraction of lines in last quarter of repo timespan | [0, 1] |
| S_n | semantic drift | `public_api_delta` per commit → `unit_normalize` | [0, 1] |
| T_c | test co-evolution | commits-with-test-cotouch / total; 1.0 for test files | [0, 1] |

V_t is `saturate_unit(x) = x/(1+|x|)` because raw coefficient-of-variation
is unbounded above. All other axes are naturally bounded.

---

## RFC-008 SignalClass (all 6 fire)

| class | rule (all thresholds v0.1) |
|---|---|
| `IncidentAftershock` | V_t > 0.3 AND commit subject matches `fix:`/`hotfix:`/`revert:` (set in scan, overrides classifier) |
| `CoupledAmplifier` | D_n < 0.3 AND C_s > 0.7 |
| `RefactorConvergence` | S_n > 0.6 AND H_a < 0.4 |
| `ApiDrift` | S_n > 0.6 AND H_a >= 0.4 |
| `OwnershipFragmentation` | H_a > 0.8 AND D_n > 0.3 |
| `FrozenNeglect` | D_n, H_a, V_t, C_s all < 0.15 |

Classifier order is most-specific-first; `IncidentAftershock` is applied
in scan before `classify()` since it depends on commit-subject data.

## Event variants (all 5 emit)

| event | fires when |
|---|---|
| `Rename` | any `ChangeKind::Renamed` during the walk |
| `Hotspot` | `detect_recent_burst(times, 0.5)` — >50% of touches in last quarter |
| `IncidentAftershock` | V_t > 0.3 + commit subject is fix/hotfix/revert |
| `OwnershipSplit` | first author held ≥2 consecutive leading commits, then new author joined |
| `ApiDrift` | signal_class classification as ApiDrift, with raw `pub_items_changed` count |

---

## Language support for S_n

| language | backend | convention |
|---|---|---|
| Rust | `syn` | `pub` visibility; tracks fn arg count, struct/enum/trait/const/static/type/use/mod/impl methods |
| Go | tree-sitter-go | uppercase first letter; fn/method/type |
| Python | tree-sitter-python | no-underscore-prefix; def/class |
| TypeScript (+ TSX) | tree-sitter-typescript | `export` keyword; function/class/interface/type_alias/const |
| Java | tree-sitter-java | `public` modifier; class/interface/enum/record/method |
| JavaScript (+ JSX/MJS/CJS extensions) | tree-sitter-javascript | ES-module `export`; function/class/const/let/var. CommonJS not tracked. |

Files with unknown extensions contribute 0 to S_n. Parsed items are cached
by `(blob_sha, Language)` so the same blob content is parsed at most once
per scan.

---

## RFC-004 lineage

`LineageResolver` (`entropyx-git::lineage`) is a union-find over rename
chains. Scan processes each commit's renames first-pass, then
canonicalizes every path before aggregating into `per_file_times`,
`per_file_authors`, `per_commit_paths`, `sn_raw`, `tc_stats`,
`incident_times`. The newer name wins as canonical, so a file's
trajectory collapses under its latest filename.

`Event::Rename` preserves literal `from`/`to` filenames; `file` FileId
resolves to the canonical trajectory row.

---

## Running

```bash
cargo test --workspace --no-fail-fast    # 204 tests at last check
cargo build --release                    # ships the binary
```

Release profile has `lto = "thin"` + `codegen-units = 1` for bitwise
determinism and max runtime perf. For iteration use:

```bash
cargo build --profile dev-release        # lto=false, codegen-units=16
```

`nice 19` + `ionice idle` on the build host prevents runaway scans from
competing with interactive workloads.

---

## Known gaps (honest v0.1 caveats)

- **`pub_items_changed` of Event::ApiDrift is a raw count**, not a per-commit stream.
- **Python class methods collapse across classes** — two classes with a same-named public method dedup to one signature. Intentional v0.1 simplification.
- **TypeScript** doesn't yet capture `export default`, re-exports, or namespace exports.
- **`entropyx-github`** is scaffolded with `pr_for_commit` only. No rate-limit retry, no review/issue endpoints, no contributor enrichment.
- **`entropyx-tq`** is not extracted — the tq1 codec lives in `entropyx-core::summary`. Extract when the codec complexity warrants a dedicated crate.
- **Release builds are LTO-heavy** — scanning combined with docker/container churn or parallel rustc can saturate the box. Docker has been purged and Claude is niced to 19 on this host, but fresh sessions should verify.

---

## Session conventions

The user paces forks with short prompts: `yes` / `proceed` / `when` / `do it`.
Reports should be tight (≤100 words for the end-of-turn summary), analytic
(exact expected values in tests rather than loose ranges), and end with
a ranked list of proposed next forks plus a stated bias.

Memory lives at `/home/ops/.claude/projects/-home-ops-Project-entropyx/memory/`.
The `project_vision.md` memory file captures the design-doc vision in
durable form; update it when the pipeline state changes materially.
