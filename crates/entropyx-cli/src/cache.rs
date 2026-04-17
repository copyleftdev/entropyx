//! Disk-backed caches for content-immutable lookups.
//!
//! Two caches live here:
//!
//!   - `DiskItemsCache` — parsed public-API items keyed by
//!     `(blob_sha, Language)`. Blob SHAs are content hashes, so the
//!     cached parse result is correct forever for the same key.
//!   - `DiskPrCache` — pull-request metadata keyed by
//!     `(owner, repo, sha)`. Once a commit is associated with a merged
//!     PR, that fact is immutable; safe to cache forever.
//!
//! Default location: `$XDG_CACHE_HOME/entropyx/`, falling back to
//! `~/.cache/entropyx/`. Override via `$ENTROPYX_CACHE_DIR`. Cache
//! files are JSON for inspectability; cache directory is created on
//! save if missing.

use entropyx_ast::Language;
use entropyx_core::PullRequestRef;
use std::collections::HashMap;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};

const ITEMS_FILE: &str = "items.json";
const PRS_FILE: &str = "prs.json";

/// Resolve the default cache directory. Returns `None` only when none
/// of `$ENTROPYX_CACHE_DIR`, `$XDG_CACHE_HOME`, or `$HOME` is set.
pub fn default_cache_dir() -> Option<PathBuf> {
    if let Ok(d) = std::env::var("ENTROPYX_CACHE_DIR") {
        return Some(PathBuf::from(d));
    }
    if let Ok(d) = std::env::var("XDG_CACHE_HOME") {
        return Some(PathBuf::from(d).join("entropyx"));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".cache").join("entropyx"))
}

fn lang_key(lang: Language) -> &'static str {
    match lang {
        Language::Rust => "rust",
        Language::Go => "go",
        Language::Python => "python",
        Language::TypeScript => "typescript",
        Language::Java => "java",
        Language::JavaScript => "javascript",
        Language::Ruby => "ruby",
        Language::Cpp => "cpp",
    }
}

/// SHA-keyed cache of parsed public-API items, persisted as JSON.
#[derive(Debug, Default)]
pub struct DiskItemsCache {
    path: PathBuf,
    map: HashMap<String, Vec<String>>,
}

impl DiskItemsCache {
    /// Load (or initialize empty) at the given file path. Missing or
    /// corrupt cache files are silently treated as empty so a stale
    /// cache never blocks a scan.
    pub fn load_at(path: PathBuf) -> Self {
        let map = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, map }
    }

    /// Convenience: load from the default cache directory.
    pub fn load_default() -> Self {
        let path = default_cache_dir()
            .map(|d| d.join(ITEMS_FILE))
            .unwrap_or_else(|| PathBuf::from(ITEMS_FILE));
        Self::load_at(path)
    }

    pub fn get(&self, sha: &str, lang: Language) -> Option<Vec<String>> {
        self.map.get(&Self::key(sha, lang)).cloned()
    }

    pub fn insert(&mut self, sha: String, lang: Language, items: Vec<String>) {
        self.map.insert(Self::key(&sha, lang), items);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    /// Persist to disk. Creates the parent directory if necessary.
    /// Errors are returned to the caller — callers typically log and
    /// proceed (a save failure shouldn't crash a successful scan).
    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(&self.map).map_err(io::Error::other)?;
        fs::write(&self.path, json)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn key(sha: &str, lang: Language) -> String {
        format!("{}/{sha}", lang_key(lang))
    }
}

/// `(owner, repo, sha)`-keyed cache of pull-request lookups, persisted
/// as JSON. `None` values are stored explicitly to record "queried,
/// no PR found" — distinct from "never queried" (cache miss).
#[derive(Debug, Default)]
pub struct DiskPrCache {
    path: PathBuf,
    map: HashMap<String, Option<PullRequestRef>>,
}

impl DiskPrCache {
    pub fn load_at(path: PathBuf) -> Self {
        let map = fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, map }
    }

    pub fn load_default() -> Self {
        let path = default_cache_dir()
            .map(|d| d.join(PRS_FILE))
            .unwrap_or_else(|| PathBuf::from(PRS_FILE));
        Self::load_at(path)
    }

    /// Three-state lookup:
    ///   - `Some(Some(pr))` — cached, PR found
    ///   - `Some(None)` — cached, no PR for this commit (don't re-query)
    ///   - `None` — not cached, caller should query the network
    pub fn get(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Option<Option<PullRequestRef>> {
        self.map.get(&Self::key(owner, repo, sha)).cloned()
    }

    pub fn insert(
        &mut self,
        owner: &str,
        repo: &str,
        sha: &str,
        pr: Option<PullRequestRef>,
    ) {
        self.map.insert(Self::key(owner, repo, sha), pr);
    }

    pub fn len(&self) -> usize {
        self.map.len()
    }

    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }

    pub fn save(&self) -> io::Result<()> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent)?;
        }
        let json = serde_json::to_string(&self.map).map_err(io::Error::other)?;
        fs::write(&self.path, json)
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    fn key(owner: &str, repo: &str, sha: &str) -> String {
        format!("{owner}/{repo}/{sha}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn items_cache_round_trips() {
        let td = tempdir().expect("tempdir");
        let path = td.path().join("items.json");

        {
            let mut c = DiskItemsCache::load_at(path.clone());
            c.insert(
                "abc123".to_string(),
                Language::Rust,
                vec!["fn:foo/2".to_string(), "struct:Bar".to_string()],
            );
            c.insert(
                "def456".to_string(),
                Language::Go,
                vec!["fn:Hello".to_string()],
            );
            c.save().expect("save");
        }

        let c = DiskItemsCache::load_at(path);
        assert_eq!(
            c.get("abc123", Language::Rust),
            Some(vec!["fn:foo/2".to_string(), "struct:Bar".to_string()]),
        );
        assert_eq!(c.get("def456", Language::Go), Some(vec!["fn:Hello".to_string()]));
        // Wrong language for an existing SHA is a miss.
        assert_eq!(c.get("abc123", Language::Go), None);
        // Unknown SHA is a miss.
        assert_eq!(c.get("missing", Language::Rust), None);
    }

    #[test]
    fn items_cache_handles_corrupt_file_as_empty() {
        let td = tempdir().expect("tempdir");
        let path = td.path().join("items.json");
        fs::write(&path, "{ not valid json").unwrap();

        let c = DiskItemsCache::load_at(path);
        assert!(c.is_empty(), "corrupt file → empty cache, no panic");
    }

    #[test]
    fn items_cache_handles_missing_file_as_empty() {
        let td = tempdir().expect("tempdir");
        let path = td.path().join("does_not_exist.json");
        let c = DiskItemsCache::load_at(path);
        assert!(c.is_empty());
    }

    #[test]
    fn items_cache_save_creates_parent_dir() {
        let td = tempdir().expect("tempdir");
        let path = td.path().join("nested").join("subdir").join("items.json");
        let mut c = DiskItemsCache::load_at(path.clone());
        c.insert("abc".to_string(), Language::Rust, vec!["fn:x/0".to_string()]);
        c.save().expect("save creates parents");
        assert!(path.exists(), "file written");
    }

    #[test]
    fn pr_cache_distinguishes_no_pr_from_unknown() {
        let td = tempdir().expect("tempdir");
        let path = td.path().join("prs.json");

        let pr = PullRequestRef {
            number: 42,
            title: "fix: thing".to_string(),
            state: "closed".to_string(),
            merged: true,
            merged_at: Some("2026-04-01T12:00:00Z".to_string()),
            author: Some("alice".to_string()),
        };

        {
            let mut c = DiskPrCache::load_at(path.clone());
            c.insert("acme", "widgets", "with_pr_sha", Some(pr.clone()));
            c.insert("acme", "widgets", "direct_push_sha", None);
            c.save().expect("save");
        }

        let c = DiskPrCache::load_at(path);
        assert_eq!(c.get("acme", "widgets", "with_pr_sha"), Some(Some(pr)));
        // Cached "no PR" → don't re-query the network.
        assert_eq!(c.get("acme", "widgets", "direct_push_sha"), Some(None));
        // Truly unknown → caller must hit network.
        assert_eq!(c.get("acme", "widgets", "unknown_sha"), None);
    }

    #[test]
    fn default_cache_dir_uses_explicit_override() {
        // SAFETY: see note in entropyx-github tests; env vars are
        // process-global and the parallel-test risk is accepted for v0.1.
        let prior = std::env::var("ENTROPYX_CACHE_DIR").ok();
        unsafe {
            std::env::set_var("ENTROPYX_CACHE_DIR", "/tmp/entropyx-test-cache");
        }
        assert_eq!(
            default_cache_dir(),
            Some(PathBuf::from("/tmp/entropyx-test-cache")),
        );
        unsafe {
            std::env::remove_var("ENTROPYX_CACHE_DIR");
        }
        if let Some(v) = prior {
            unsafe {
                std::env::set_var("ENTROPYX_CACHE_DIR", v);
            }
        }
    }
}
