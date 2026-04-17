use crate::{BlameLine, ChangeKind, CommitMeta, FileChange, Result, Signature};
use std::path::{Path, PathBuf};
use std::process::Command;

/// Thin facade over `gix::Repository`. All methods that cross the facade
/// boundary return owned, entropyx-native types; gix types do not leak.
pub struct Repo {
    inner: gix::Repository,
}

impl Repo {
    pub fn open(path: impl AsRef<Path>) -> Result<Self> {
        let inner = gix::open(path.as_ref())?;
        Ok(Self { inner })
    }

    pub fn git_dir(&self) -> PathBuf {
        self.inner.git_dir().to_path_buf()
    }

    /// Working-tree root for non-bare repositories. `None` indicates a
    /// bare repo (no checkout to blame against).
    pub fn work_dir(&self) -> Option<PathBuf> {
        self.inner.work_dir().map(Path::to_path_buf)
    }

    /// Infer the GitHub `owner/name` slug from the repo's `origin`
    /// remote URL. Returns `None` when there's no origin, the URL isn't
    /// a GitHub form we recognize, or the repository is bare.
    ///
    /// Recognized URL shapes:
    ///   - `https://github.com/owner/name[.git]`
    ///   - `http://github.com/owner/name[.git]`
    ///   - `git@github.com:owner/name[.git]`
    ///   - `ssh://git@github.com/owner/name[.git]`
    pub fn github_slug(&self) -> Option<String> {
        let work_dir = self.work_dir()?;
        let out = Command::new("git")
            .args(["config", "--get", "remote.origin.url"])
            .current_dir(&work_dir)
            .output()
            .ok()?;
        if !out.status.success() {
            return None;
        }
        let url = std::str::from_utf8(&out.stdout).ok()?.trim();
        parse_github_slug(url)
    }

    /// Blame a file at HEAD and return one `BlameLine` per source line,
    /// in file order.
    ///
    /// v0.1 shells out to `git blame --line-porcelain`. gix 0.66's
    /// umbrella crate does not expose a stable blame API yet. Migrating
    /// to `gix-blame` when it graduates will not change this signature.
    pub fn blame(&self, file_path: &str) -> Result<Vec<BlameLine>> {
        let work_dir = self
            .work_dir()
            .ok_or("repo has no working tree (bare repo?)")?;
        let out = Command::new("git")
            .args(["blame", "--line-porcelain", "--", file_path])
            .current_dir(&work_dir)
            .output()?;
        if !out.status.success() {
            return Err(format!(
                "git blame failed for {file_path}: {}",
                String::from_utf8_lossy(&out.stderr),
            )
            .into());
        }
        let text =
            std::str::from_utf8(&out.stdout).map_err(|e| format!("blame output not UTF-8: {e}"))?;
        Ok(crate::blame::parse_line_porcelain(text))
    }

    pub fn has_commits(&self) -> bool {
        self.inner.head_id().is_ok()
    }

    /// Read HEAD and return its commit metadata as an owned value.
    /// `Ok(None)` indicates an unborn HEAD (fresh repo, no commits) —
    /// an expected state, not an error.
    pub fn head_commit_meta(&self) -> Result<Option<CommitMeta>> {
        let head_id = match self.inner.head_id() {
            Ok(id) => id,
            Err(_) => return Ok(None),
        };
        Ok(Some(self.commit_meta(head_id.detach())?))
    }

    /// Walk history starting from HEAD, newest first by committer time.
    /// Each yielded item is an owned `CommitMeta`; inter-item errors
    /// (corrupt object, missing parent) are surfaced per-item.
    pub fn walk(&self) -> Result<impl Iterator<Item = Result<CommitMeta>> + '_> {
        let head_id = self.inner.head_id()?;
        let walk = self
            .inner
            .rev_walk([head_id.detach()])
            .sorting(gix::traverse::commit::simple::Sorting::ByCommitTimeNewestFirst)
            .all()?;
        Ok(walk.map(move |step| {
            let info = step?;
            self.commit_meta(info.id)
        }))
    }

    /// Resolve `(commit_sha, path)` to the blob's git SHA — the cheap
    /// half of `blob_at`, for callers that want to key a cache without
    /// paying to read blob data. Returns `Ok(None)` when the path is
    /// absent or isn't a blob entry.
    pub fn blob_sha_at(&self, commit_sha: &str, path: &str) -> Result<Option<String>> {
        let id = gix::ObjectId::from_hex(commit_sha.as_bytes())?;
        let commit = self.inner.find_object(id)?.try_into_commit()?;
        let mut tree = commit.tree()?;
        let Some(entry) = tree.peel_to_entry_by_path(std::path::Path::new(path))? else {
            return Ok(None);
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        Ok(Some(entry.oid().to_string()))
    }

    /// Fetch a blob's UTF-8 content by its git SHA directly, skipping
    /// path resolution. Pairs with `blob_sha_at` for SHA-keyed caching.
    /// Returns `Ok(None)` for missing object, non-blob kind, or non-UTF-8.
    pub fn blob_by_sha(&self, sha: &str) -> Result<Option<String>> {
        let oid = gix::ObjectId::from_hex(sha.as_bytes())?;
        let object = self.inner.find_object(oid)?;
        if object.kind != gix::object::Kind::Blob {
            return Ok(None);
        }
        Ok(std::str::from_utf8(&object.data).ok().map(str::to_string))
    }

    /// Fetch the UTF-8 blob content for `path` at the tree of `commit_sha`.
    /// Returns `Ok(None)` when the path doesn't exist at that commit, the
    /// entry isn't a blob (e.g. submodule, symlink), or the blob isn't
    /// valid UTF-8. Binary files are silently skipped — callers that
    /// need raw bytes can add a sibling method later.
    pub fn blob_at(&self, commit_sha: &str, path: &str) -> Result<Option<String>> {
        let id = gix::ObjectId::from_hex(commit_sha.as_bytes())?;
        let commit = self.inner.find_object(id)?.try_into_commit()?;
        let mut tree = commit.tree()?;
        let Some(entry) = tree.peel_to_entry_by_path(std::path::Path::new(path))? else {
            return Ok(None);
        };
        if !entry.mode().is_blob() {
            return Ok(None);
        }
        let oid = entry.oid();
        let object = self.inner.find_object(oid)?;
        Ok(std::str::from_utf8(&object.data).ok().map(str::to_string))
    }

    /// Look up a single commit by its 40-char hex SHA and return owned
    /// metadata. Mirrors `head_commit_meta()` but for an arbitrary commit.
    pub fn commit_by_sha(&self, sha: &str) -> Result<CommitMeta> {
        let id = gix::ObjectId::from_hex(sha.as_bytes())?;
        self.commit_meta(id)
    }

    /// Walk commits reachable from `head_sha` but not from `base_sha` —
    /// the canonical `git log base..head` set. Newest-first by committer
    /// time. Behaves correctly over branching/merging histories because
    /// the filter prunes `base` and its ancestry entirely via gix's
    /// `selected()` traversal predicate.
    pub fn walk_range<'a>(
        &'a self,
        base_sha: &str,
        head_sha: &str,
    ) -> Result<impl Iterator<Item = Result<CommitMeta>> + 'a> {
        let base_id = gix::ObjectId::from_hex(base_sha.as_bytes())?;
        let head_id = gix::ObjectId::from_hex(head_sha.as_bytes())?;
        let walk = self
            .inner
            .rev_walk([head_id])
            .sorting(gix::traverse::commit::simple::Sorting::ByCommitTimeNewestFirst)
            .selected(move |oid| oid != base_id.as_ref())?;
        Ok(walk.map(move |step| {
            let info = step?;
            self.commit_meta(info.id)
        }))
    }

    /// Compute the file-level diff from `from_sha` to `to_sha`. Both SHAs
    /// are full 40-char hex commit ids. Rename detection is enabled with
    /// gix's default thresholds; results are sorted by new-tree path so
    /// repeated calls are bitwise-identical (RFC-001).
    pub fn diff(&self, from_sha: &str, to_sha: &str) -> Result<Vec<FileChange>> {
        let from_id = gix::ObjectId::from_hex(from_sha.as_bytes())?;
        let to_id = gix::ObjectId::from_hex(to_sha.as_bytes())?;
        let from_tree = self.inner.find_object(from_id)?.try_into_commit()?.tree()?;
        let to_tree = self.inner.find_object(to_id)?.try_into_commit()?.tree()?;
        iter_diff(&from_tree, &to_tree)
    }

    /// Diff a commit against its first parent, treating the empty tree
    /// as the "from" side for root commits. This is the conventional
    /// "what did this commit introduce?" view and is what D_n (RFC-007)
    /// wants per-commit.
    pub fn diff_from_parent(&self, sha: &str) -> Result<Vec<FileChange>> {
        let id = gix::ObjectId::from_hex(sha.as_bytes())?;
        let commit = self.inner.find_object(id)?.try_into_commit()?;
        let to_tree = commit.tree()?;

        let from_tree_owned;
        let from_tree_ref: &gix::Tree<'_>;
        let empty;

        match commit.parent_ids().next() {
            Some(parent_id) => {
                // In a shallow clone, the parent SHA is recorded in the
                // commit object but the parent OBJECT is not present.
                // gix raises "object … could not be found". Treat a
                // missing parent as if this commit were a root — its
                // diff becomes the full tree added. This matches git's
                // own behavior at a shallow boundary.
                match self.inner.find_object(parent_id.detach()) {
                    Ok(obj) => {
                        from_tree_owned = obj.try_into_commit()?.tree()?;
                        from_tree_ref = &from_tree_owned;
                    }
                    Err(gix::object::find::existing::Error::NotFound { .. }) => {
                        empty = self.inner.empty_tree();
                        from_tree_ref = &empty;
                    }
                    Err(e) => return Err(e.into()),
                }
            }
            None => {
                empty = self.inner.empty_tree();
                from_tree_ref = &empty;
            }
        }

        iter_diff(from_tree_ref, &to_tree)
    }

    /// Walk HEAD's tree and return `(path, blob_sha)` for every blob
    /// present. Output is sorted by path for deterministic downstream
    /// consumption (RFC-001). An unborn HEAD yields an empty vec, not
    /// an error.
    ///
    /// This powers content-addressed handles in the tq1 Summary: each
    /// `file` entry's blob SHA becomes the stable `Handle::file` key
    /// that AI consumers pass back to `entropyx explain`.
    pub fn head_tree_entries(&self) -> Result<Vec<(String, String)>> {
        let head_id = match self.inner.head_id() {
            Ok(id) => id,
            Err(_) => return Ok(Vec::new()),
        };
        let tree = self
            .inner
            .find_object(head_id.detach())?
            .try_into_commit()?
            .tree()?;
        let entries = tree.traverse().breadthfirst.files()?;
        let mut out: Vec<(String, String)> = entries
            .into_iter()
            .filter(|e| e.mode.is_blob())
            .map(|e| {
                let path = String::from_utf8_lossy(&e.filepath).into_owned();
                (path, e.oid.to_string())
            })
            .collect();
        out.sort_by(|a, b| a.0.cmp(&b.0));
        Ok(out)
    }

    fn commit_meta(&self, id: gix::ObjectId) -> Result<CommitMeta> {
        let commit = self.inner.find_object(id)?.try_into_commit()?;
        let decoded = commit.decode()?;

        Ok(CommitMeta {
            sha: id.to_string(),
            parents: decoded.parents().map(|p| p.to_string()).collect(),
            tree: decoded.tree().to_string(),
            author: sig_from(&decoded.author),
            committer: sig_from(&decoded.committer),
            subject: subject_of(decoded.message),
        })
    }
}

fn iter_diff(from_tree: &gix::Tree<'_>, to_tree: &gix::Tree<'_>) -> Result<Vec<FileChange>> {
    let mut changes: Vec<FileChange> = Vec::new();
    let mut platform = from_tree.changes()?;
    platform
        .track_path()
        .track_rewrites(Some(gix::diff::Rewrites::default()));

    platform.for_each_to_obtain_tree(to_tree, |change| {
        // Skip directory-level events — we want per-blob granularity so
        // aggregations key on full file paths, not tree prefixes. gix
        // recurses into trees automatically; this just filters their
        // intermediate Addition/Deletion records.
        if !change.event.entry_mode().is_blob() {
            return Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue);
        }
        let path = String::from_utf8_lossy(change.location).into_owned();
        let kind = match change.event {
            gix::object::tree::diff::change::Event::Addition { .. } => ChangeKind::Added,
            gix::object::tree::diff::change::Event::Deletion { .. } => ChangeKind::Deleted,
            gix::object::tree::diff::change::Event::Modification { .. } => ChangeKind::Modified,
            gix::object::tree::diff::change::Event::Rewrite {
                source_location,
                diff,
                copy,
                ..
            } => {
                let from = String::from_utf8_lossy(source_location).into_owned();
                let similarity = diff
                    .map(|d| {
                        let total = d.removals + d.insertions;
                        if total == 0 {
                            100
                        } else {
                            100u32.saturating_sub(total.min(100))
                        }
                    })
                    .unwrap_or(100);
                if copy {
                    ChangeKind::Copied { from, similarity }
                } else {
                    ChangeKind::Renamed { from, similarity }
                }
            }
        };
        changes.push(FileChange { path, kind });
        Ok::<_, std::convert::Infallible>(gix::object::tree::diff::Action::Continue)
    })?;

    changes.sort_by(|a, b| a.path.cmp(&b.path));
    Ok(changes)
}

/// Extract the `owner/name` slug from a GitHub remote URL. Returns
/// `None` when the URL isn't one of the recognized GitHub shapes or
/// doesn't contain two path segments.
pub fn parse_github_slug(url: &str) -> Option<String> {
    let url = url.trim().trim_end_matches(".git").trim_end_matches('/');
    let rest = [
        "https://github.com/",
        "http://github.com/",
        "ssh://git@github.com/",
    ]
    .iter()
    .find_map(|p| url.strip_prefix(p))
    .or_else(|| url.strip_prefix("git@github.com:"))?;
    // Take exactly the first two path segments: owner/name. Paths with
    // trailing segments (sub-paths, branches) are rejected via this
    // two-segment slice.
    let mut parts = rest.splitn(3, '/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if owner.is_empty() || name.is_empty() {
        return None;
    }
    Some(format!("{owner}/{name}"))
}

fn sig_from(s: &gix::actor::SignatureRef<'_>) -> Signature {
    Signature {
        name: bstr_to_string(s.name),
        email: bstr_to_string(s.email),
        time: s.time.seconds,
    }
}

fn bstr_to_string(b: &gix::bstr::BStr) -> String {
    String::from_utf8_lossy(b).into_owned()
}

fn subject_of(message: &gix::bstr::BStr) -> String {
    let bytes: &[u8] = message;
    let end = bytes
        .iter()
        .position(|&c| c == b'\n')
        .unwrap_or(bytes.len());
    String::from_utf8_lossy(&bytes[..end]).into_owned()
}

#[cfg(test)]
mod parse_tests {
    use super::parse_github_slug;

    #[test]
    fn https_url_with_dot_git() {
        assert_eq!(
            parse_github_slug("https://github.com/acme/widgets.git"),
            Some("acme/widgets".to_string()),
        );
    }

    #[test]
    fn https_url_without_dot_git() {
        assert_eq!(
            parse_github_slug("https://github.com/acme/widgets"),
            Some("acme/widgets".to_string()),
        );
    }

    #[test]
    fn ssh_style_with_colon() {
        assert_eq!(
            parse_github_slug("git@github.com:acme/widgets.git"),
            Some("acme/widgets".to_string()),
        );
    }

    #[test]
    fn ssh_proto_url() {
        assert_eq!(
            parse_github_slug("ssh://git@github.com/acme/widgets.git"),
            Some("acme/widgets".to_string()),
        );
    }

    #[test]
    fn trailing_slash_is_tolerated() {
        assert_eq!(
            parse_github_slug("https://github.com/acme/widgets/"),
            Some("acme/widgets".to_string()),
        );
    }

    #[test]
    fn non_github_hosts_are_rejected() {
        assert_eq!(parse_github_slug("https://gitlab.com/acme/widgets"), None,);
        assert_eq!(
            parse_github_slug("https://bitbucket.org/acme/widgets"),
            None,
        );
    }

    #[test]
    fn missing_segments_are_rejected() {
        assert_eq!(parse_github_slug("https://github.com/acme"), None);
        assert_eq!(parse_github_slug("https://github.com/"), None);
        assert_eq!(parse_github_slug("https://github.com"), None);
        assert_eq!(parse_github_slug(""), None);
    }

    #[test]
    fn sub_paths_are_collapsed_to_owner_name() {
        // Some tools add fragments; we still resolve the first two segments.
        assert_eq!(
            parse_github_slug("https://github.com/acme/widgets/tree/main"),
            Some("acme/widgets".to_string()),
        );
    }
}
