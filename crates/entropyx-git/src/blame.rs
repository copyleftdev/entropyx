//! Blame surface. v0.1 shells out to `git blame --line-porcelain` because
//! gix 0.66's umbrella crate does not expose a stable blame feature yet;
//! gix-blame is standalone and its API will land here without changing
//! this module's public shape.

use serde::{Deserialize, Serialize};

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct BlameLine {
    /// 1-based line number in the final file.
    pub line_number: u32,
    /// Full 40-char hex SHA of the commit that last touched this line.
    pub commit_sha: String,
    /// Unix epoch seconds (author time). Timezone dropped for RFC-001.
    pub author_time: i64,
}

/// Parse `git blame --line-porcelain` output into one `BlameLine` per
/// source line. Each line in the input alternates between a header
/// block (sha + metadata) and a content line (tab-prefixed). We use
/// `--line-porcelain` specifically so every block carries full metadata
/// — no cross-line state about previously-seen commits.
pub(crate) fn parse_line_porcelain(text: &str) -> Vec<BlameLine> {
    let mut out = Vec::new();
    let mut cur_sha = String::new();
    let mut cur_line: u32 = 0;
    let mut cur_time: i64 = 0;

    for line in text.lines() {
        if line.starts_with('\t') {
            // Content line — commit the accumulated block as one BlameLine.
            if !cur_sha.is_empty() {
                out.push(BlameLine {
                    line_number: cur_line,
                    commit_sha: cur_sha.clone(),
                    author_time: cur_time,
                });
            }
        } else if is_header_line(line) {
            // Header line: `<sha> <orig-line> <final-line> [num-lines]`
            let mut parts = line.splitn(4, ' ');
            cur_sha = parts.next().unwrap_or("").to_string();
            let _orig = parts.next();
            cur_line = parts.next().and_then(|s| s.parse().ok()).unwrap_or(0);
            cur_time = 0;
        } else if let Some(t) = line.strip_prefix("author-time ") {
            cur_time = t.parse().unwrap_or(0);
        }
        // All other metadata lines (author, committer, summary, etc.)
        // are intentionally ignored — v0.1 B_y only needs per-line time.
    }

    out
}

/// A header line starts with a full 40-char hex SHA followed by a space.
/// Necessary because other porcelain lines (`filename …`, `author-time …`)
/// also start with hex letters — a naive first-char check would misfire.
fn is_header_line(line: &str) -> bool {
    let bytes = line.as_bytes();
    bytes.len() >= 41 && bytes[40] == b' ' && bytes[..40].iter().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_line_block() {
        let txt = "\
abcdef0000000000000000000000000000000000 1 1
author Alice
author-mail <a@ex.com>
author-time 1700000000
author-tz +0000
committer Alice
committer-mail <a@ex.com>
committer-time 1700000000
committer-tz +0000
summary first
filename foo.rs
\thello
";
        let got = parse_line_porcelain(txt);
        assert_eq!(got.len(), 1);
        assert_eq!(got[0].line_number, 1);
        assert_eq!(
            got[0].commit_sha,
            "abcdef0000000000000000000000000000000000"
        );
        assert_eq!(got[0].author_time, 1700000000);
    }

    #[test]
    fn parses_multiple_lines_from_different_commits() {
        let txt = "\
aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa 1 1
author A
author-time 100
filename x
\tline1
bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb 2 2
author B
author-time 200
filename x
\tline2
";
        let got = parse_line_porcelain(txt);
        assert_eq!(got.len(), 2);
        assert_eq!(got[0].line_number, 1);
        assert_eq!(got[0].author_time, 100);
        assert_eq!(got[1].line_number, 2);
        assert_eq!(got[1].author_time, 200);
        assert_ne!(got[0].commit_sha, got[1].commit_sha);
    }

    #[test]
    fn empty_input_yields_empty_output() {
        assert!(parse_line_porcelain("").is_empty());
    }
}
