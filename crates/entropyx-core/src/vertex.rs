//! Shared vertex table for the four graphs of RFC-006.
//!
//! All four graphs (commit DAG, file lineage, co-change, ownership) share
//! interned identifiers that index into this table. Interning is
//! insertion-ordered: the first time a key is seen, a fresh id is minted,
//! and repeat lookups return the same id. This gives us O(1) join semantics
//! across graphs without schema translation.

use crate::id::{AuthorId, CommitId, FileId};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Default, Serialize, Deserialize)]
pub struct VertexTable {
    /// 40-char lowercase-hex git object hashes in first-seen order.
    pub commits: Vec<String>,
    /// Lineage keys in first-seen order. **Not paths.** See RFC-004.
    pub files: Vec<String>,
    /// Normalized author identities (typically lowercased email).
    pub authors: Vec<String>,

    #[serde(skip)]
    commit_ix: BTreeMap<String, u32>,
    #[serde(skip)]
    file_ix: BTreeMap<String, u32>,
    #[serde(skip)]
    author_ix: BTreeMap<String, u32>,
}

impl VertexTable {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn intern_commit(&mut self, sha: &str) -> CommitId {
        CommitId(Self::intern(&mut self.commits, &mut self.commit_ix, sha))
    }

    pub fn intern_file(&mut self, lineage_key: &str) -> FileId {
        FileId(Self::intern(
            &mut self.files,
            &mut self.file_ix,
            lineage_key,
        ))
    }

    pub fn intern_author(&mut self, normalized: &str) -> AuthorId {
        AuthorId(Self::intern(
            &mut self.authors,
            &mut self.author_ix,
            normalized,
        ))
    }

    fn intern(store: &mut Vec<String>, index: &mut BTreeMap<String, u32>, key: &str) -> u32 {
        if let Some(&id) = index.get(key) {
            return id;
        }
        let id: u32 = store.len().try_into().expect("id space exhausted");
        assert!(id != u32::MAX, "id space exhausted");
        store.push(key.to_owned());
        index.insert(key.to_owned(), id);
        id
    }

    /// Rebuild in-memory indexes after deserialization. Required because the
    /// indexes are `#[serde(skip)]` to keep the on-wire envelope minimal.
    pub fn rehydrate(&mut self) {
        self.commit_ix = self.commits.iter().cloned().zip(0u32..).collect();
        self.file_ix = self.files.iter().cloned().zip(0u32..).collect();
        self.author_ix = self.authors.iter().cloned().zip(0u32..).collect();
    }

    pub fn commit(&self, id: CommitId) -> Option<&str> {
        self.commits.get(id.index()).map(String::as_str)
    }
    pub fn file(&self, id: FileId) -> Option<&str> {
        self.files.get(id.index()).map(String::as_str)
    }
    pub fn author(&self, id: AuthorId) -> Option<&str> {
        self.authors.get(id.index()).map(String::as_str)
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
    pub fn author_count(&self) -> usize {
        self.authors.len()
    }
    pub fn commit_count(&self) -> usize {
        self.commits.len()
    }
}
