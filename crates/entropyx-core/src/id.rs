//! Interned identity types.
//!
//! A `FileId` denotes a *persistent trajectory* across renames/splits/merges
//! (RFC-004). It is not a path. Callers obtain `FileId` values from
//! `VertexTable::intern_file` keyed by a lineage resolver's output, not by
//! a raw path. The resolver itself lives in `entropyx-git`.

use serde::{Deserialize, Serialize};

macro_rules! define_id {
    ($(#[$meta:meta])* $name:ident) => {
        $(#[$meta])*
        #[derive(
            Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd,
            Serialize, Deserialize,
        )]
        #[serde(transparent)]
        pub struct $name(pub u32);

        impl $name {
            /// Reserved null sentinel. Do not construct directly in callers.
            pub const NULL: Self = Self(u32::MAX);

            #[inline]
            pub fn index(self) -> usize { self.0 as usize }

            #[inline]
            pub fn is_null(self) -> bool { self.0 == u32::MAX }
        }
    };
}

define_id! {
    /// Persistent-trajectory file identity (RFC-004).
    FileId
}
define_id! {
    /// Interned commit identity; maps to a git object hash via `VertexTable`.
    CommitId
}
define_id! {
    /// Identity-normalized author (RFC-010 — email lowercased, aliases merged).
    AuthorId
}

/// Seconds since the UNIX epoch. Always an explicit parameter — no crate in
/// the workspace reads the system clock during scoring (RFC-001).
#[derive(Copy, Clone, Debug, Eq, PartialEq, Hash, Ord, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Timestamp(pub i64);

impl Timestamp {
    pub const EPOCH: Self = Self(0);

    #[inline]
    pub fn seconds_since(self, earlier: Timestamp) -> i64 {
        self.0 - earlier.0
    }
}

/// Confidence in lineage resolution for a given `FileId`, in `[0, 1]`.
/// Emitted alongside every file-keyed metric so consumers can filter
/// low-confidence rows.
#[derive(Copy, Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LineageConfidence(pub f64);

impl LineageConfidence {
    pub const EXACT: Self = Self(1.0);
    pub const UNKNOWN: Self = Self(0.0);
}
