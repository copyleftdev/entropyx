//! RFC-004 rename-chain lineage resolver.
//!
//! A file's *path* changes over time but its *trajectory* does not.
//! When we compute per-file metrics, renames must collapse into a
//! single identity or every rename fragments history — D_n drops at
//! the rename, authorship stats split, the composite misleads.
//!
//! This module implements a union-find over rename events. Callers
//! `union(old_path, new_path)` once per observed rename; thereafter
//! `canonical(path)` returns the representative name for the
//! trajectory. Rename chains (A→B→C) resolve correctly.
//!
//! **Canonical-name convention**: the last path in the call order
//! becomes root. If callers pass unions in reverse-chronological order
//! (newest-first walk), the canonical ends up as the newest name —
//! what users see today. Processing in chronological order would make
//! the oldest name canonical instead; both are valid choices.

use std::collections::HashMap;

#[derive(Debug, Default)]
pub struct LineageResolver {
    index: HashMap<String, usize>,
    paths: Vec<String>,
    parent: Vec<usize>,
}

impl LineageResolver {
    pub fn new() -> Self {
        Self::default()
    }

    /// Register a path without creating any union. Safe to call
    /// multiple times; idempotent.
    pub fn intern(&mut self, path: &str) -> usize {
        if let Some(&i) = self.index.get(path) {
            return i;
        }
        let i = self.paths.len();
        self.paths.push(path.to_string());
        self.parent.push(i);
        self.index.insert(path.to_string(), i);
        i
    }

    /// Record a rename: `old_path` and `new_path` share a trajectory.
    /// The second argument wins as the root. In a newest-first walk,
    /// callers pass `union(from, to)` which makes the newer name the
    /// canonical representative.
    pub fn union(&mut self, old_path: &str, new_path: &str) {
        let a = self.intern(old_path);
        let b = self.intern(new_path);
        let ra = self.find(a);
        let rb = self.find(b);
        if ra != rb {
            self.parent[ra] = rb;
        }
    }

    /// Canonical trajectory name for `path`. Returns the path as an
    /// owned `String`; if the path was never interned, it's returned
    /// unchanged (safe fallback for never-renamed files).
    pub fn canonical(&mut self, path: &str) -> String {
        let Some(&i) = self.index.get(path) else {
            return path.to_string();
        };
        let root = self.find(i);
        self.paths[root].clone()
    }

    /// All paths that share a trajectory with `path`, sorted. Returns
    /// `[path]` if the path has no aliases or isn't interned.
    pub fn aliases(&mut self, path: &str) -> Vec<String> {
        let Some(&i) = self.index.get(path) else {
            return vec![path.to_string()];
        };
        let root = self.find(i);
        let n = self.paths.len();
        // Materialize roots in one pass so we can iterate `paths` later
        // without re-borrowing `self` from inside a filter/map closure.
        let roots: Vec<usize> = (0..n).map(|j| self.find(j)).collect();
        let mut out: Vec<String> = roots
            .iter()
            .enumerate()
            .filter(|&(_, &r)| r == root)
            .map(|(j, _)| self.paths[j].clone())
            .collect();
        out.sort();
        out
    }

    fn find(&mut self, mut x: usize) -> usize {
        // Path compression: flatten chains as we traverse them.
        while self.parent[x] != x {
            self.parent[x] = self.parent[self.parent[x]];
            x = self.parent[x];
        }
        x
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unseen_path_returns_itself() {
        let mut r = LineageResolver::new();
        assert_eq!(r.canonical("anything.rs"), "anything.rs");
    }

    #[test]
    fn interned_but_unrenamed_path_is_its_own_canonical() {
        let mut r = LineageResolver::new();
        r.intern("solo.rs");
        assert_eq!(r.canonical("solo.rs"), "solo.rs");
        assert_eq!(r.aliases("solo.rs"), vec!["solo.rs"]);
    }

    #[test]
    fn single_rename_both_sides_canonicalize_to_new() {
        let mut r = LineageResolver::new();
        r.union("old.rs", "new.rs");
        assert_eq!(r.canonical("old.rs"), "new.rs");
        assert_eq!(r.canonical("new.rs"), "new.rs");
    }

    #[test]
    fn chain_of_renames_collapses_to_last() {
        // Walk order: newest→oldest, so unions arrive as:
        //   c4: b → c (union("b", "c"))
        //   c2: a → b (union("a", "b"))
        let mut r = LineageResolver::new();
        r.union("b.rs", "c.rs"); // newest rename first
        r.union("a.rs", "b.rs"); // older rename second
        assert_eq!(r.canonical("a.rs"), "c.rs");
        assert_eq!(r.canonical("b.rs"), "c.rs");
        assert_eq!(r.canonical("c.rs"), "c.rs");
        let mut aliases = r.aliases("a.rs");
        aliases.sort();
        assert_eq!(aliases, vec!["a.rs", "b.rs", "c.rs"]);
    }

    #[test]
    fn unrelated_files_stay_separate() {
        let mut r = LineageResolver::new();
        r.union("old.rs", "new.rs");
        r.intern("other.rs");
        assert_eq!(r.canonical("other.rs"), "other.rs");
        assert_eq!(r.aliases("other.rs"), vec!["other.rs"]);
        // And new.rs doesn't accidentally pick up other.rs.
        let mut aliases = r.aliases("new.rs");
        aliases.sort();
        assert_eq!(aliases, vec!["new.rs", "old.rs"]);
    }

    #[test]
    fn idempotent_intern_does_not_duplicate() {
        let mut r = LineageResolver::new();
        let a = r.intern("x.rs");
        let b = r.intern("x.rs");
        assert_eq!(a, b);
        assert_eq!(r.aliases("x.rs"), vec!["x.rs"]);
    }

    #[test]
    fn union_same_path_is_noop() {
        let mut r = LineageResolver::new();
        r.union("same.rs", "same.rs");
        assert_eq!(r.canonical("same.rs"), "same.rs");
        assert_eq!(r.aliases("same.rs"), vec!["same.rs"]);
    }

    #[test]
    fn multiple_chains_do_not_interfere() {
        // Two independent rename chains.
        let mut r = LineageResolver::new();
        r.union("a1.rs", "a2.rs");
        r.union("b1.rs", "b2.rs");
        r.union("a2.rs", "a3.rs"); // extend chain a
        assert_eq!(r.canonical("a1.rs"), "a3.rs");
        assert_eq!(r.canonical("a2.rs"), "a3.rs");
        assert_eq!(r.canonical("a3.rs"), "a3.rs");
        assert_eq!(r.canonical("b1.rs"), "b2.rs");
        assert_eq!(r.canonical("b2.rs"), "b2.rs");
        // Cross-chain independence.
        let mut a_aliases = r.aliases("a1.rs");
        a_aliases.sort();
        assert_eq!(a_aliases, vec!["a1.rs", "a2.rs", "a3.rs"]);
        let mut b_aliases = r.aliases("b1.rs");
        b_aliases.sort();
        assert_eq!(b_aliases, vec!["b1.rs", "b2.rs"]);
    }
}
