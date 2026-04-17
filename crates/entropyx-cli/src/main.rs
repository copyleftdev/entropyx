//! entropyx CLI.
//!
//! v0.1 commands:
//!   - `describe` — self-identifying protocol root (RFC-000, RFC-009).
//!   - `scan`     — walk a local repo and emit a tq1 `Summary` envelope
//!                  whose per-file rows carry D_n, H_a, V_t, C_s, and a
//!                  composite score against RFC-007 default weights.
//!                  B_y (blame youth), S_n (semantic drift), T_c (test
//!                  co-evolution) are reported as 0.0 until their
//!                  subsystems (blame, entropyx-ast) land.

use entropyx_core::metric::{
    author_dispersion, author_entropy_nats, blame_youth, calibrate, change_counts, classify,
    detect_ownership_split, detect_recent_burst, is_incident_subject, is_test_path, saturate_unit,
    temporal_volatility, unit_normalize, CalibrationConfig,
};
use entropyx_cli::cache::{DiskItemsCache, DiskPrCache};
use entropyx_core::{
    Handle, MetricComponents, ScoreWeights, SignalClass, Timestamp, VertexTable,
};
use entropyx_tq::{Dict, Enrichments, Event, FileRow, Schema, Summary};
use entropyx_graph::CoChangeGraph;
use std::collections::BTreeMap;
use std::io::Write;
use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    match args.first().map(String::as_str) {
        Some("describe") => describe(&args[1..]),
        Some("scan") => scan(&args[1..]),
        Some("explain") => explain(&args[1..]),
        Some("calibrate") => calibrate_cmd(&args[1..]),
        Some("--version") => {
            println!("entropyx {}", env!("CARGO_PKG_VERSION"));
            ExitCode::SUCCESS
        }
        Some("--help") | Some("-h") | None => {
            print_usage();
            ExitCode::SUCCESS
        }
        Some(other) => {
            eprintln!("entropyx: unknown command `{other}`");
            print_usage();
            ExitCode::from(2)
        }
    }
}

fn describe(args: &[String]) -> ExitCode {
    let format = args
        .iter()
        .position(|a| a == "--format")
        .and_then(|i| args.get(i + 1))
        .map(String::as_str)
        .unwrap_or("json");

    let d = entropyx_core::Describe::current();
    let mut stdout = std::io::stdout().lock();
    match format {
        "json" => {
            if let Err(e) = serde_json::to_writer_pretty(&mut stdout, &d) {
                eprintln!("describe: {e}");
                return ExitCode::FAILURE;
            }
            let _ = stdout.write_all(b"\n");
            ExitCode::SUCCESS
        }
        other => {
            eprintln!("describe: unsupported --format `{other}` (use json)");
            ExitCode::from(2)
        }
    }
}

fn scan(args: &[String]) -> ExitCode {
    // Flags: `--weights <path>` (calibrated weights), `--github
    // [owner/name]` (enrich with PR metadata; bare form auto-detects
    // slug from origin remote), `--no-cache` (disable disk caches for
    // this run — every blob is reparsed, every PR re-queried).
    let mut weights_path: Option<String> = None;
    let mut github: Option<Option<String>> = None;
    let mut no_cache = false;
    let mut positional: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        match a.as_str() {
            "--weights" => {
                match args.get(i + 1) {
                    Some(p) => {
                        weights_path = Some(p.clone());
                        i += 2;
                    }
                    None => {
                        eprintln!("entropyx scan: --weights requires a path");
                        return ExitCode::from(2);
                    }
                }
            }
            "--github" => {
                let next_is_slug = args
                    .get(i + 1)
                    .map(|v| v.contains('/') && !v.starts_with("--"))
                    .unwrap_or(false);
                if next_is_slug {
                    github = Some(Some(args[i + 1].clone()));
                    i += 2;
                } else {
                    github = Some(None);
                    i += 1;
                }
            }
            "--no-cache" => {
                no_cache = true;
                i += 1;
            }
            _ => {
                positional.push(a);
                i += 1;
            }
        }
    }

    let Some(path) = positional.first() else {
        eprintln!("entropyx scan: missing <path>");
        return ExitCode::from(2);
    };
    // Drop the intermediate `&&String` layer so subsequent code sees `&str`.
    let path: &str = path.as_str();

    // Load custom weights if supplied, otherwise use the RFC-007 defaults.
    let weights = match weights_path {
        Some(wp) => match std::fs::read_to_string(&wp) {
            Ok(s) => match serde_json::from_str::<ScoreWeights>(&s) {
                Ok(w) => w,
                Err(e) => {
                    eprintln!("entropyx scan: invalid weights JSON at {wp}: {e}");
                    return ExitCode::FAILURE;
                }
            },
            Err(e) => {
                eprintln!("entropyx scan: cannot read {wp}: {e}");
                return ExitCode::FAILURE;
            }
        },
        None => MetricComponents::DEFAULT_WEIGHTS,
    };

    let repo = match entropyx_git::Repo::open(path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("entropyx scan: open failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let walk = match repo.walk() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("entropyx scan: walk init failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let metas: Vec<_> = match walk.collect::<Result<Vec<_>, _>>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("entropyx scan: walk failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Aggregate per-file from diff_from_parent (empty tree for root commits).
    // Each tuple is (committer_time, commit_sha) so events can attach a
    // SHA to every emitted signal — enables joining with
    // Summary.enrichments.pull_requests for PR context on every event.
    let mut per_file_times: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
    let mut per_file_authors: BTreeMap<String, Vec<String>> = BTreeMap::new();
    let mut per_commit_paths: Vec<Vec<String>> = Vec::with_capacity(metas.len());
    // (from_path, to_path, at_time, sha) captured during the walk,
    // resolved to FileIds + events once interning completes.
    let mut rename_raw: Vec<(String, String, i64, String)> = Vec::new();
    // Per-file accumulated public-API delta across the walk — raw count,
    // normalized later into S_n ∈ [0, 1].
    let mut sn_raw: BTreeMap<String, u64> = BTreeMap::new();
    // RFC-004 lineage resolver. Renames are recorded as we walk (newest-
    // first, so newer names win as canonical roots); subsequent path
    // references collapse pre- and post-rename history onto the same
    // trajectory. Every aggregation key below is a *canonical* path,
    // not a raw filename — so a file's metrics survive renames intact.
    let mut lineage = entropyx_git::LineageResolver::new();
    // (time, sha) of incident-tagged commits (fix/hotfix/revert
    // subjects) touching each file. Feeds the IncidentAftershock rule
    // and — when --github is set — the PR-enrichment join.
    let mut incident_times: BTreeMap<String, Vec<(i64, String)>> = BTreeMap::new();
    // Per-file (commits_touching, commits_with_test_cotouch) counts.
    // T_c = cotouch / touching for non-test files; 1.0 for test files.
    let mut tc_stats: BTreeMap<String, (u64, u64)> = BTreeMap::new();
    // Blob-SHA-keyed cache of parsed public-API items. With caching
    // enabled (the default), loads from $XDG_CACHE_HOME/entropyx/
    // items.json and saves at scan end — second runs absorb nearly all
    // parse cost. `--no-cache` skips both load and save (useful when
    // debugging classifier output or after upstream changes).
    let mut items_cache = if no_cache {
        DiskItemsCache::default()
    } else {
        DiskItemsCache::load_default()
    };

    for commit in &metas {
        let changes = match repo.diff_from_parent(&commit.sha) {
            Ok(c) => c,
            Err(e) => {
                eprintln!("entropyx scan: diff failed at {}: {e}", commit.sha);
                return ExitCode::FAILURE;
            }
        };

        let parent_sha = commit.parents.first().map(String::as_str);
        let commit_is_incident = is_incident_subject(&commit.subject);
        let commit_has_test = changes.iter().any(|c| is_test_path(&c.path));

        // First pass: register renames so every subsequent path lookup
        // this commit (and earlier ones in the walk) sees the unified
        // trajectory.
        for ch in &changes {
            if let entropyx_git::ChangeKind::Renamed { from, .. } = &ch.kind {
                lineage.union(from, &ch.path);
            }
        }

        // Second pass: aggregate under the canonical trajectory name.
        for ch in &changes {
            let canonical = lineage.canonical(&ch.path);

            if let entropyx_git::ChangeKind::Renamed { from, .. } = &ch.kind {
                // Literal from/to survive in the event — callers that
                // want the rename story see the actual filename pair.
                rename_raw.push((
                    from.clone(),
                    ch.path.clone(),
                    commit.committer.time,
                    commit.sha.clone(),
                ));
            }
            if commit_is_incident {
                incident_times
                    .entry(canonical.clone())
                    .or_default()
                    .push((commit.committer.time, commit.sha.clone()));
            }

            // S_n: public-API delta for every change whose path maps to
            // a supported language. Unknown extensions contribute 0.
            // Item lists are cached by (blob_sha, language) so any blob
            // appearing as both "new-side at C" and "old-side at C's
            // child" is parsed exactly once across the whole walk.
            if let Some(lang) = entropyx_ast::language_from_path(&ch.path) {
                let old_side = match &ch.kind {
                    entropyx_git::ChangeKind::Renamed { from, .. }
                    | entropyx_git::ChangeKind::Copied { from, .. } => from.as_str(),
                    _ => ch.path.as_str(),
                };
                let old_items =
                    cached_items(&repo, parent_sha, old_side, lang, &mut items_cache);
                let new_items = if matches!(&ch.kind, entropyx_git::ChangeKind::Deleted) {
                    Vec::new()
                } else {
                    cached_items(
                        &repo,
                        Some(&commit.sha),
                        &ch.path,
                        lang,
                        &mut items_cache,
                    )
                };
                let delta =
                    entropyx_ast::public_api_delta_from_items(&old_items, &new_items)
                        as u64;
                *sn_raw.entry(canonical).or_insert(0) += delta;
            }
        }

        // Deduplicated canonical path set for this commit. Used for
        // per_file_times/authors accumulation and as the co-change graph
        // edge set.
        let mut canonical_paths: Vec<String> = changes
            .into_iter()
            .map(|c| lineage.canonical(&c.path))
            .collect();
        canonical_paths.sort();
        canonical_paths.dedup();
        for path in &canonical_paths {
            per_file_times
                .entry(path.clone())
                .or_default()
                .push((commit.committer.time, commit.sha.clone()));
            per_file_authors
                .entry(path.clone())
                .or_default()
                .push(commit.author.email.clone());
            // T_c stats: every touch counts in the denominator; cotouch
            // with a test-file in the same commit counts in the numerator.
            // Test files themselves are excluded from the numerator — we
            // give them 1.0 at row-build time.
            let stats = tc_stats.entry(path.clone()).or_insert((0u64, 0u64));
            stats.0 += 1;
            if commit_has_test && !is_test_path(path) {
                stats.1 += 1;
            }
        }
        per_commit_paths.push(canonical_paths);
    }

    // Normalize raw counts into [0, 1].
    let dn = unit_normalize(&change_counts(&per_commit_paths));
    let sn = unit_normalize(&sn_raw);

    // Co-change graph feeds C_s as max(normalized-weighted-degree,
    // betweenness). Degree rewards "files that co-change often";
    // betweenness rewards "files that bridge otherwise-disconnected
    // subgraphs". A file whose removal would partition the graph
    // surfaces via betweenness even when its raw degree is modest.
    let graph = CoChangeGraph::from_commit_paths(&per_commit_paths);
    let mut degree_raw: BTreeMap<String, u64> = BTreeMap::new();
    for node in graph.nodes() {
        degree_raw.insert(node.clone(), graph.weighted_degree(node));
    }
    let degree_norm = unit_normalize(&degree_raw);
    let betweenness = graph.betweenness_centrality();
    let cs: BTreeMap<String, f64> = degree_norm
        .iter()
        .map(|(path, &d)| {
            let b = *betweenness.get(path).unwrap_or(&0.0);
            (path.clone(), d.max(b))
        })
        .collect();

    // Intern paths and emails in deterministic order (BTreeMap keys +
    // metas-newest-first give stable, reproducible FileIds/AuthorIds).
    let mut vt = VertexTable::new();
    for path in per_file_times.keys() {
        vt.intern_file(path);
    }
    for commit in &metas {
        vt.intern_author(&commit.author.email);
    }

    // HEAD tree: needed for B_y (blame-youth requires a file at HEAD)
    // and reused for handle minting at the bottom.
    let head_entries: BTreeMap<String, String> = match repo.head_tree_entries() {
        Ok(v) => v.into_iter().collect(),
        Err(e) => {
            eprintln!("entropyx scan: head tree walk failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Repository-wide time span for B_y normalization. Using the same
    // [first, last] for every file is load-bearing: per-file bounds
    // would collapse every file into a self-relative window and erase
    // cross-file comparison.
    let repo_first = metas.iter().map(|m| m.committer.time).min().unwrap_or(0);
    let repo_last = metas.iter().map(|m| m.committer.time).max().unwrap_or(0);

    // Blame every file still present at HEAD. Failures (e.g. binary
    // files we can't blame) degrade to B_y=0 rather than aborting the
    // scan — the rest of the pipeline is still useful.
    let mut by_map: BTreeMap<String, f64> = BTreeMap::new();
    for path in head_entries.keys() {
        if let Ok(lines) = repo.blame(path) {
            let times: Vec<i64> = lines.iter().map(|l| l.author_time).collect();
            by_map.insert(path.clone(), blame_youth(&times, repo_first, repo_last));
        }
    }

    let mut rows: Vec<FileRow> = Vec::with_capacity(per_file_times.len());
    // ApiDrift emissions captured during the row loop (path, at, sha, raw)
    // so they can be promoted to Events once the VertexTable is settled.
    let mut api_drift_emissions: Vec<(String, i64, String, u32)> = Vec::new();
    for (path, times) in &per_file_times {
        let authors = &per_file_authors[path];
        let fid = vt.intern_file(path);
        let d_n = *dn.get(path).unwrap_or(&0.0);
        let h_a = author_dispersion(authors);
        // Per-touch times without the SHA — the physics primitives only
        // need the scalars. Materializing once keeps the three call
        // sites below cheap.
        let raw_times: Vec<i64> = times.iter().map(|(t, _)| *t).collect();
        // V_t is a coefficient of variation — unbounded above — so squash
        // it into [0, 1) before feeding the weighted-sum composite.
        // Ranking is preserved; absolute scale is now meaningful.
        let v_t = saturate_unit(temporal_volatility(&raw_times));
        let c_s = *cs.get(path).unwrap_or(&0.0);
        let b_y = *by_map.get(path).unwrap_or(&0.0);
        let s_n = *sn.get(path).unwrap_or(&0.0);
        // T_c: test files get 1.0 (they ARE the test surface). Non-test
        // files get fraction of their touching-commits that also touched
        // a test file in the same commit — "tests move with the code".
        let t_c = if is_test_path(path) {
            1.0
        } else {
            let (total, cotouch) = tc_stats.get(path).copied().unwrap_or((0, 0));
            if total > 0 { cotouch as f64 / total as f64 } else { 0.0 }
        };
        let components = MetricComponents {
            change_density: d_n,
            author_entropy: h_a,
            temporal_volatility: v_t,
            coupling_stress: c_s,
            blame_youth: b_y,
            semantic_drift: s_n,
            test_cooevolution: t_c,
        };
        let composite = components.composite(weights);
        // IncidentAftershock overrides static classification: a file with
        // bursty cadence (V_t > 0.3) touched by any incident-tagged commit
        // is in active firefighting territory, regardless of what its
        // other dimensions look like.
        let in_aftershock = v_t > 0.3
            && incident_times.get(path).map_or(false, |v| !v.is_empty());
        let signal_class = if in_aftershock {
            Some(SignalClass::IncidentAftershock)
        } else {
            classify(&components)
        };
        if signal_class == Some(SignalClass::ApiDrift) {
            // Emit at the latest touch; carry its SHA for PR join.
            let latest = times.iter().max_by_key(|(t, _)| *t);
            let (at, sha) = match latest {
                Some((t, s)) => (*t, s.clone()),
                None => (0, String::new()),
            };
            let raw = sn_raw.get(path).copied().unwrap_or(0) as u32;
            api_drift_emissions.push((path.clone(), at, sha, raw));
        }
        rows.push(FileRow {
            file: fid,
            values: [d_n, h_a, v_t, c_s, b_y, s_n, t_c, composite],
            lineage_confidence: 1.0,
            signal_class,
        });
    }

    // Resolve rename events against the now-populated VertexTable.
    // Renames intern the *canonical* path's FileId (so the event joins
    // back to the trajectory's FileRow under RFC-004 lineage), while
    // `from`/`to` preserve the literal filenames from the rename commit.
    let mut events: Vec<Event> = rename_raw
        .iter()
        .map(|(from, to, at, sha)| {
            let canonical_to = lineage.canonical(to);
            Event::Rename {
                file: vt.intern_file(&canonical_to),
                at: Timestamp(*at),
                sha: sha.clone(),
                from: from.clone(),
                to: to.clone(),
            }
        })
        .collect();

    // Hotspot events: emit one per file that satisfies the recent-burst
    // rule. Threshold 0.5 means "majority of touches landed in the last
    // quarter of observed time span" — narrative-worthy, not noisy.
    const HOTSPOT_THRESHOLD: f64 = 0.5;
    for (path, times) in &per_file_times {
        let raw_times: Vec<i64> = times.iter().map(|(t, _)| *t).collect();
        if let Some(at) = detect_recent_burst(&raw_times, HOTSPOT_THRESHOLD) {
            let sha = times
                .iter()
                .find(|(t, _)| *t == at)
                .map(|(_, s)| s.clone())
                .unwrap_or_default();
            events.push(Event::Hotspot {
                file: vt.intern_file(path),
                at: Timestamp(at),
                sha,
                reason: "recent_burst".to_string(),
            });
        }
    }

    // OwnershipSplit events: for each file, walk its touches in
    // chronological order and detect the moment a single-author regime
    // gave way to multiple contributors (bus-factor expansion).
    for (path, times) in &per_file_times {
        let authors = match per_file_authors.get(path) {
            Some(a) => a,
            None => continue,
        };
        let mut chrono: Vec<(i64, &str)> = times
            .iter()
            .zip(authors.iter())
            .map(|((t, _), a)| (*t, a.as_str()))
            .collect();
        chrono.sort_by_key(|(t, _)| *t);
        if let Some((at, split_authors)) = detect_ownership_split(&chrono) {
            let author_ids = split_authors
                .into_iter()
                .map(|a| vt.intern_author(a))
                .collect();
            let sha = times
                .iter()
                .find(|(t, _)| *t == at)
                .map(|(_, s)| s.clone())
                .unwrap_or_default();
            events.push(Event::OwnershipSplit {
                file: vt.intern_file(path),
                at: Timestamp(at),
                sha,
                authors: author_ids,
            });
        }
    }

    // IncidentAftershock events: for each file with V_t > 0.3 AND at
    // least one incident-tagged commit, emit the aftershock window.
    const AFTERSHOCK_VT_THRESHOLD: f64 = 0.3;
    for (path, inc_times) in &incident_times {
        if inc_times.is_empty() {
            continue;
        }
        let Some(times) = per_file_times.get(path) else {
            continue;
        };
        // Use the same saturated V_t as the row output so threshold
        // semantics line up with what downstream consumers see.
        let raw_times: Vec<i64> = times.iter().map(|(t, _)| *t).collect();
        let v_t = saturate_unit(temporal_volatility(&raw_times));
        if v_t <= AFTERSHOCK_VT_THRESHOLD {
            continue;
        }
        let latest = inc_times.iter().max_by_key(|(t, _)| *t).unwrap();
        let at = latest.0;
        let sha = latest.1.clone();
        let first = inc_times.iter().map(|(t, _)| *t).min().unwrap();
        let window_days = ((at - first) / 86_400).max(0) as u32;
        events.push(Event::IncidentAftershock {
            file: vt.intern_file(path),
            at: Timestamp(at),
            sha,
            window_days,
        });
    }

    // ApiDrift events: promote the per-file classifications captured in
    // the row loop. `pub_items_changed` is the accumulated raw API delta
    // across the walk — a concrete count that pairs with the qualitative
    // classification so AI consumers can reason about magnitude.
    for (path, at, sha, pub_items_changed) in api_drift_emissions {
        events.push(Event::ApiDrift {
            file: vt.intern_file(&path),
            at: Timestamp(at),
            sha,
            pub_items_changed,
        });
    }

    // Mint content-addressed handles for every scanned file that still
    // exists at HEAD (renamed-away / deleted files have no current blob
    // to hash, so they don't get handles — their FileRow survives).
    // `head_entries` was populated earlier for B_y and is reused here.
    let mut handles: BTreeMap<String, Handle> = BTreeMap::new();
    for path in per_file_times.keys() {
        if let Some(blob_sha) = head_entries.get(path) {
            let fid = vt.intern_file(path);
            let handle = Handle::file(fid, blob_sha);
            handles.insert(handle.key(), handle);
        }
    }

    // Optional GitHub enrichment: fetch PR metadata for every event
    // that carries a SHA (currently only IncidentAftershock). Collects
    // unique SHAs so the same commit isn't queried twice.
    let mut enrichments = Enrichments::default();
    if let Some(requested) = github {
        let slug = match requested {
            Some(s) => s,
            None => match repo.github_slug() {
                Some(s) => s,
                None => {
                    eprintln!(
                        "entropyx scan: --github auto-detect found no github remote; \
                         pass `--github owner/name` explicitly",
                    );
                    return ExitCode::FAILURE;
                }
            },
        };
        let Some((owner, repo_name)) = slug.split_once('/') else {
            eprintln!("entropyx scan: --github expects owner/name (got {slug})");
            return ExitCode::from(2);
        };
        use entropyx_github::GithubClient;
        let client = entropyx_github::HttpClient::from_env();
        let mut pr_cache = if no_cache {
            DiskPrCache::default()
        } else {
            DiskPrCache::load_default()
        };
        let mut seen = std::collections::BTreeSet::new();
        for ev in &events {
            // Every event variant carries a `sha` field (empty when the
            // emitter couldn't determine one). Extract it uniformly.
            let sha = match ev {
                Event::Rename { sha, .. }
                | Event::Hotspot { sha, .. }
                | Event::OwnershipSplit { sha, .. }
                | Event::ApiDrift { sha, .. }
                | Event::IncidentAftershock { sha, .. } => sha,
            };
            if sha.is_empty() || !seen.insert(sha.clone()) {
                continue;
            }

            // Disk cache check first — three-state: cached PR, cached
            // "no PR for this commit", or unknown (network query needed).
            let pr = match pr_cache.get(owner, repo_name, sha) {
                Some(cached) => cached,
                None => match client.pr_for_commit(owner, repo_name, sha) {
                    Ok(pr) => {
                        pr_cache.insert(owner, repo_name, sha, pr.clone());
                        pr
                    }
                    Err(e) => {
                        eprintln!(
                            "entropyx scan: github lookup failed for {sha}: {e}",
                        );
                        return ExitCode::FAILURE;
                    }
                },
            };
            if let Some(pr) = pr {
                enrichments.pull_requests.insert(sha.clone(), pr);
            }
        }
        if !no_cache {
            if let Err(e) = pr_cache.save() {
                eprintln!("entropyx scan: warning — could not save PR cache: {e}");
            }
        }
    }

    // Persist the items cache unless --no-cache disabled it. New parses
    // accumulated this run become available to the next.
    if !no_cache {
        if let Err(e) = items_cache.save() {
            eprintln!("entropyx scan: warning — could not save items cache: {e}");
        }
    }

    let summary = Summary {
        schema: Schema::default(),
        dict: Dict::from_vertex(&vt),
        files: rows,
        events,
        handles,
        enrichments,
    };

    let mut stdout = std::io::stdout().lock();
    if let Err(e) = serde_json::to_writer_pretty(&mut stdout, &summary) {
        eprintln!("entropyx scan: {e}");
        return ExitCode::FAILURE;
    }
    let _ = stdout.write_all(b"\n");
    ExitCode::SUCCESS
}

fn explain(args: &[String]) -> ExitCode {
    // `--github` is optional:
    //   - absent: no enrichment
    //   - bare (`--github` with no slug or next arg doesn't look like one):
    //     auto-detect slug from the repo's origin remote URL
    //   - `--github owner/name`: explicit override
    //
    // Detection rule: a token containing `/` and not starting with `--`
    // following `--github` is treated as its value; anything else is
    // treated as a positional arg (and --github is a bare switch).
    //
    // `Some(None)` = enrich, auto-detect; `Some(Some(slug))` = enrich,
    // explicit; `None` = don't enrich.
    let mut github: Option<Option<String>> = None;
    let mut positional: Vec<&String> = Vec::new();
    let mut i = 0;
    while i < args.len() {
        let a = &args[i];
        if a == "--github" {
            let next_is_slug = args
                .get(i + 1)
                .map(|v| v.contains('/') && !v.starts_with("--"))
                .unwrap_or(false);
            if next_is_slug {
                github = Some(Some(args[i + 1].clone()));
                i += 2;
            } else {
                github = Some(None);
                i += 1;
            }
        } else {
            positional.push(a);
            i += 1;
        }
    }
    let Some(repo_path) = positional.first() else {
        eprintln!("entropyx explain: missing <repo-path>");
        return ExitCode::from(2);
    };
    let Some(key_or_path) = positional.get(1) else {
        eprintln!("entropyx explain: missing <handle-key | file-path>");
        return ExitCode::from(2);
    };

    let repo = match entropyx_git::Repo::open(repo_path) {
        Ok(r) => r,
        Err(e) => {
            eprintln!("entropyx explain: open failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Dispatch on handle kind. The three canonical handle namespaces —
    // `file:`, `commit:`, `range:` — mirror the Handle enum in
    // entropyx-core. Anything else is interpreted as a raw file path,
    // which keeps ergonomic `explain <repo> src/foo.rs` working.
    if let Some(sha) = key_or_path.strip_prefix("commit:") {
        // Resolve Auto → concrete slug via the repo's origin remote.
        let resolved: Option<String> = match &github {
            None => None,
            Some(Some(slug)) => Some(slug.clone()),
            Some(None) => match repo.github_slug() {
                Some(s) => Some(s),
                None => {
                    eprintln!(
                        "entropyx explain: --github auto-detect found no github remote; \
                         pass `--github owner/name` explicitly",
                    );
                    return ExitCode::FAILURE;
                }
            },
        };
        return explain_commit(&repo, sha, resolved.as_deref());
    }
    if let Some(rest) = key_or_path.strip_prefix("range:") {
        let Some((base, head)) = rest.split_once("..") else {
            eprintln!(
                "entropyx explain: malformed range handle — expected `range:<base>..<head>`",
            );
            return ExitCode::from(2);
        };
        return explain_range(&repo, base, head);
    }
    if let Some(prefix) = key_or_path.strip_prefix("file:") {
        let entries = match repo.head_tree_entries() {
            Ok(v) => v,
            Err(e) => {
                eprintln!("entropyx explain: head tree walk failed: {e}");
                return ExitCode::FAILURE;
            }
        };
        let Some((path, _)) = entries.iter().find(|(_, sha)| sha.starts_with(prefix))
        else {
            eprintln!(
                "entropyx explain: handle {key_or_path} matches no blob at HEAD",
            );
            return ExitCode::FAILURE;
        };
        return explain_file(&repo, path);
    }
    explain_file(&repo, key_or_path)
}

fn explain_file(repo: &entropyx_git::Repo, file_path: &str) -> ExitCode {
    let walk = match repo.walk() {
        Ok(w) => w,
        Err(e) => {
            eprintln!("entropyx explain: walk init failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let metas: Vec<_> = match walk.collect::<Result<Vec<_>, _>>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("entropyx explain: walk failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Filter the walk to commits that touch the requested path. Renames
    // on either side count as a touch so the AI gets a complete view
    // regardless of which name it asks about.
    let mut evidence: Vec<serde_json::Value> = Vec::new();
    let mut times: Vec<i64> = Vec::new();
    let mut authors: Vec<String> = Vec::new();

    for commit in &metas {
        let changes = match repo.diff_from_parent(&commit.sha) {
            Ok(c) => c,
            Err(e) => {
                eprintln!(
                    "entropyx explain: diff failed at {}: {e}",
                    commit.sha
                );
                return ExitCode::FAILURE;
            }
        };
        let touched = changes.iter().any(|c| {
            c.path == file_path || c.previous_path() == Some(file_path)
        });
        if !touched {
            continue;
        }
        times.push(commit.committer.time);
        authors.push(commit.author.email.clone());
        evidence.push(serde_json::json!({
            "sha": commit.sha,
            "subject": commit.subject,
            "author": commit.author.email,
            "time": commit.committer.time,
        }));
    }

    let mut counts: BTreeMap<&str, u64> = BTreeMap::new();
    for a in &authors {
        *counts.entry(a.as_str()).or_insert(0) += 1;
    }
    let total = authors.len() as f64;
    let mut ranked: Vec<(&&str, &u64)> = counts.iter().collect();
    ranked.sort_by(|a, b| b.1.cmp(a.1).then(a.0.cmp(b.0)));
    let top_authors: Vec<_> = ranked
        .iter()
        .take(5)
        .map(|(email, count)| {
            let share = if total > 0.0 {
                (**count as f64) / total
            } else {
                0.0
            };
            serde_json::json!({ "email": *email, "share": share })
        })
        .collect();

    let report = serde_json::json!({
        "schema": {
            "name": "entropyx-explain",
            "version": entropyx_core::CONTRACT_VERSION,
        },
        "kind": "file",
        "path": file_path,
        "commits_touched": times.len(),
        "first_commit_time": times.iter().min().copied(),
        "last_commit_time": times.iter().max().copied(),
        "top_authors": top_authors,
        "metrics": {
            "change_count": times.len(),
            "author_entropy_nats": author_entropy_nats(&authors),
            "author_dispersion": author_dispersion(&authors),
            "temporal_volatility": temporal_volatility(&times),
        },
        "commits": evidence,
    });

    write_json(&report, "explain")
}

fn explain_commit(
    repo: &entropyx_git::Repo,
    sha: &str,
    github_slug: Option<&str>,
) -> ExitCode {
    let meta = match repo.commit_by_sha(sha) {
        Ok(m) => m,
        Err(e) => {
            eprintln!("entropyx explain: commit {sha} not found: {e}");
            return ExitCode::FAILURE;
        }
    };
    let changes = match repo.diff_from_parent(&meta.sha) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("entropyx explain: diff failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    let mut renames = 0usize;
    let changes_json: Vec<serde_json::Value> = changes
        .iter()
        .map(|c| {
            let kind_str = match &c.kind {
                entropyx_git::ChangeKind::Added => "added",
                entropyx_git::ChangeKind::Deleted => "deleted",
                entropyx_git::ChangeKind::Modified => "modified",
                entropyx_git::ChangeKind::Renamed { .. } => {
                    renames += 1;
                    "renamed"
                }
                entropyx_git::ChangeKind::Copied { .. } => "copied",
            };
            let from = c.previous_path().map(str::to_string);
            serde_json::json!({
                "path": c.path,
                "kind": kind_str,
                "from": from,
            })
        })
        .collect();

    let mut report = serde_json::json!({
        "schema": {
            "name": "entropyx-explain",
            "version": entropyx_core::CONTRACT_VERSION,
        },
        "kind": "commit",
        "commit": {
            "sha": meta.sha,
            "subject": meta.subject,
            "tree": meta.tree,
            "parents": meta.parents,
            "author": {
                "name": meta.author.name,
                "email": meta.author.email,
                "time": meta.author.time,
            },
            "committer": {
                "name": meta.committer.name,
                "email": meta.committer.email,
                "time": meta.committer.time,
            },
        },
        "changes": changes_json,
        "stats": {
            "files_changed": changes.len(),
            "renames": renames,
        },
    });

    // Optional GitHub enrichment: `--github owner/name` attaches the
    // PR that introduced this commit, if any. Requires network; uses
    // GITHUB_TOKEN env var when set (rate limit 5000/hr vs 60/hr
    // unauthenticated).
    if let Some(slug) = github_slug {
        let Some((owner, repo_name)) = slug.split_once('/') else {
            eprintln!(
                "entropyx explain: --github expects owner/name (got {slug})",
            );
            return ExitCode::from(2);
        };
        use entropyx_github::GithubClient;
        let client = entropyx_github::HttpClient::from_env();
        match client.pr_for_commit(owner, repo_name, &meta.sha) {
            Ok(Some(pr)) => {
                report
                    .as_object_mut()
                    .unwrap()
                    .insert(
                        "pull_request".to_string(),
                        serde_json::to_value(&pr).unwrap(),
                    );
            }
            Ok(None) => {
                // No PR references the commit — typically a direct push.
                // Record explicitly rather than omitting so callers can
                // distinguish "no PR" from "never queried".
                report
                    .as_object_mut()
                    .unwrap()
                    .insert("pull_request".to_string(), serde_json::Value::Null);
            }
            Err(e) => {
                eprintln!("entropyx explain: github lookup failed: {e}");
                return ExitCode::FAILURE;
            }
        }
    }

    write_json(&report, "explain")
}

fn explain_range(repo: &entropyx_git::Repo, base: &str, head: &str) -> ExitCode {
    let walk = match repo.walk_range(base, head) {
        Ok(w) => w,
        Err(e) => {
            eprintln!("entropyx explain: range walk init failed: {e}");
            return ExitCode::FAILURE;
        }
    };
    let metas: Vec<_> = match walk.collect::<Result<Vec<_>, _>>() {
        Ok(v) => v,
        Err(e) => {
            eprintln!("entropyx explain: range walk failed: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Aggregate file paths touched across the range, plus per-commit summaries.
    let mut files_touched: std::collections::BTreeSet<String> = Default::default();
    let mut distinct_authors: std::collections::BTreeSet<String> = Default::default();
    let mut commit_entries: Vec<serde_json::Value> = Vec::with_capacity(metas.len());
    let mut first_t: Option<i64> = None;
    let mut last_t: Option<i64> = None;

    for commit in &metas {
        distinct_authors.insert(commit.author.email.clone());
        let t = commit.committer.time;
        first_t = Some(first_t.map_or(t, |cur| cur.min(t)));
        last_t = Some(last_t.map_or(t, |cur| cur.max(t)));

        match repo.diff_from_parent(&commit.sha) {
            Ok(changes) => {
                for ch in changes {
                    files_touched.insert(ch.path);
                }
            }
            Err(e) => {
                eprintln!(
                    "entropyx explain: range diff failed at {}: {e}",
                    commit.sha
                );
                return ExitCode::FAILURE;
            }
        }

        commit_entries.push(serde_json::json!({
            "sha": commit.sha,
            "subject": commit.subject,
            "author": commit.author.email,
            "time": t,
        }));
    }

    let report = serde_json::json!({
        "schema": {
            "name": "entropyx-explain",
            "version": entropyx_core::CONTRACT_VERSION,
        },
        "kind": "range",
        "range": { "base": base, "head": head },
        "commit_count": metas.len(),
        "distinct_authors": distinct_authors.len(),
        "first_commit_time": first_t,
        "last_commit_time": last_t,
        "files_touched": files_touched.iter().collect::<Vec<_>>(),
        "commits": commit_entries,
    });

    write_json(&report, "explain")
}

/// SHA-keyed cache lookup for parsed public-API items at
/// `(commit_sha, path)` for a given language. On miss, resolves the
/// blob SHA, fetches the content, parses, and stores. A missing commit,
/// missing path, non-UTF-8 blob, or parse failure all collapse to an
/// empty item list (the safe identity for `public_api_delta_from_items`).
///
/// Cache is keyed on `(blob_sha, language)` and persisted to disk via
/// `DiskItemsCache` — repeated scans of the same repo skip the parse
/// entirely on hits.
fn cached_items(
    repo: &entropyx_git::Repo,
    commit_sha: Option<&str>,
    path: &str,
    lang: entropyx_ast::Language,
    cache: &mut DiskItemsCache,
) -> Vec<String> {
    let Some(commit_sha) = commit_sha else {
        return Vec::new();
    };
    let Some(sha) = repo.blob_sha_at(commit_sha, path).ok().flatten() else {
        return Vec::new();
    };
    if let Some(items) = cache.get(&sha, lang) {
        return items;
    }
    let content = repo.blob_by_sha(&sha).ok().flatten().unwrap_or_default();
    let items = entropyx_ast::parse_public_items(&content, lang).unwrap_or_default();
    cache.insert(sha, lang, items.clone());
    items
}

fn write_json(value: &serde_json::Value, ctx: &str) -> ExitCode {
    let mut stdout = std::io::stdout().lock();
    if let Err(e) = serde_json::to_writer_pretty(&mut stdout, value) {
        eprintln!("entropyx {ctx}: {e}");
        return ExitCode::FAILURE;
    }
    let _ = stdout.write_all(b"\n");
    ExitCode::SUCCESS
}

fn calibrate_cmd(args: &[String]) -> ExitCode {
    let mut summary_path: Option<String> = None;
    let mut labels_path: Option<String> = None;
    let mut iter = args.iter();
    while let Some(a) = iter.next() {
        match a.as_str() {
            "--summary" => summary_path = iter.next().cloned(),
            "--labels" => labels_path = iter.next().cloned(),
            other => {
                eprintln!("entropyx calibrate: unknown argument `{other}`");
                return ExitCode::from(2);
            }
        }
    }
    let Some(summary_path) = summary_path else {
        eprintln!("entropyx calibrate: missing --summary <path>");
        return ExitCode::from(2);
    };
    let Some(labels_path) = labels_path else {
        eprintln!("entropyx calibrate: missing --labels <path>");
        return ExitCode::from(2);
    };

    let summary: Summary = match std::fs::read_to_string(&summary_path)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
    {
        Ok(s) => s,
        Err(e) => {
            eprintln!("entropyx calibrate: reading {summary_path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    let labels: BTreeMap<String, f64> = match std::fs::read_to_string(&labels_path)
        .map_err(|e| e.to_string())
        .and_then(|s| serde_json::from_str(&s).map_err(|e| e.to_string()))
    {
        Ok(l) => l,
        Err(e) => {
            eprintln!("entropyx calibrate: reading {labels_path}: {e}");
            return ExitCode::FAILURE;
        }
    };

    // Pair rows with labels. Composite column (index 7) is excluded —
    // only the 7 input features feed the fit.
    let mut features: Vec<[f64; 7]> = Vec::new();
    let mut label_vec: Vec<f64> = Vec::new();
    for row in &summary.files {
        let idx = row.file.index();
        let path = match summary.dict.files.get(idx) {
            Some(p) => p,
            None => continue,
        };
        if let Some(&label) = labels.get(path) {
            features.push([
                row.values[0],
                row.values[1],
                row.values[2],
                row.values[3],
                row.values[4],
                row.values[5],
                row.values[6],
            ]);
            label_vec.push(label);
        }
    }

    if features.is_empty() {
        eprintln!(
            "entropyx calibrate: no files match between summary and labels — \
             emitting DEFAULT_WEIGHTS",
        );
    }

    let fitted = calibrate(&features, &label_vec, CalibrationConfig::default());
    write_json(&serde_json::to_value(&fitted).unwrap(), "calibrate")
}

fn print_usage() {
    eprintln!(
        "entropyx — forensic instrument for codebase dynamics

Usage:
  entropyx describe [--format json]           self-describing protocol root
  entropyx scan <path> [--weights <file>] [--github [owner/name]] [--no-cache]
                                              walk a repo, emit a tq1 Summary.
                                              --github enriches events with PR
                                              context. --no-cache bypasses the
                                              disk cache for fresh parses.
  entropyx explain <repo-path> <handle | file-path> [--github owner/name]
                                              per-file / commit / range evidence
                                              (--github enriches commit: with PR)
  entropyx calibrate --summary <file> --labels <file>
                                              fit ScoreWeights via RFC-012 ridge
  entropyx --version
  entropyx --help
"
    );
}
