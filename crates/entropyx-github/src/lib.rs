//! Sparse GitHub enricher for entropyx.
//!
//! The design-doc vision: local Git is the truth source, GitHub is a
//! selective enricher. That means the v0.1 surface is deliberately
//! narrow — we fetch only what the local walk cannot derive.
//!
//! This turn exposes one enrichment: `pr_for_commit`. Given a commit
//! SHA, return the Pull Request that introduced it (if any). The AI
//! narrative layer uses this to answer "what review context exists
//! for this commit?" — something git blame alone can't tell you.
//!
//! The `GithubClient` trait lets callers swap the network-backed
//! `HttpClient` for a `MockClient` in tests, so the crate is
//! fully unit-testable without internet access.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

pub use entropyx_core::PullRequestRef;

pub type Error = Box<dyn std::error::Error + Send + Sync + 'static>;
pub type Result<T> = std::result::Result<T, Error>;

/// Maximum retries per request. Shared budget across rate-limit waits
/// and transient 5xx backoff — keeps scans failing fast rather than
/// stacking long sleeps.
const MAX_RETRIES: u32 = 3;

/// Maximum single-retry wait, in seconds. Caps the damage when
/// `X-RateLimit-Reset` points to a multi-hour window (can happen on
/// unauthenticated 60/hr quota refilling at the top of the hour).
const MAX_RETRY_WAIT_SECS: u64 = 300;

/// Abstract over GitHub access so tests can mock the network.
pub trait GithubClient {
    /// Fetch the pull request that introduced the commit identified by
    /// `sha`. Returns `Ok(None)` when no PR references the commit —
    /// that's a valid forensic fact, not an error. Returns `Err` on
    /// network failure, HTTP 4xx/5xx, rate-limit exhaustion, or malformed
    /// response.
    fn pr_for_commit(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<Option<PullRequestRef>>;
}

/// Real HTTP client against api.github.com. Caches successful lookups
/// by `(owner, repo, sha)` — commit↔PR associations are immutable for
/// merged commits, so the cache can live for the process lifetime.
///
/// Rate-limit handling: on 429 or 403+`X-RateLimit-Remaining: 0`, the
/// client sleeps until `X-RateLimit-Reset` (or `Retry-After`, capped at
/// `MAX_RETRY_WAIT_SECS`) and retries up to `MAX_RATE_LIMIT_RETRIES`
/// times. Progress goes to stderr so users can see why a scan paused.
pub struct HttpClient {
    agent: ureq::Agent,
    token: Option<String>,
    cache: Mutex<HashMap<String, Option<PullRequestRef>>>,
}

impl HttpClient {
    pub fn new(token: Option<String>) -> Self {
        // Disable the default "4xx/5xx as error" behavior so we can
        // inspect rate-limit headers on 403/429 responses manually.
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .http_status_as_error(false)
            .timeout_global(Some(Duration::from_secs(30)))
            .build()
            .into();
        Self {
            agent,
            token,
            cache: Mutex::new(HashMap::new()),
        }
    }

    /// Construct from the `GITHUB_TOKEN` environment variable. Returns
    /// an unauthenticated client if the env var is unset (rate limits
    /// drop to 60/hr, so tokens are strongly recommended).
    pub fn from_env() -> Self {
        Self::new(std::env::var("GITHUB_TOKEN").ok())
    }

    fn cache_key(owner: &str, repo: &str, sha: &str) -> String {
        format!("{owner}/{repo}/{sha}")
    }
}

/// Extract a retry-wait duration from rate-limit response headers.
/// Prefers `Retry-After` (seconds), falls back to `X-RateLimit-Reset`
/// (unix epoch), defaults to 60 seconds. `now` is passed in so the
/// function is testable without clock mocking.
fn retry_wait_seconds_from_headers(
    headers: &ureq::http::HeaderMap,
    now_unix: u64,
) -> u64 {
    if let Some(secs) = headers
        .get("Retry-After")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        return secs;
    }
    if let Some(reset) = headers
        .get("X-RateLimit-Reset")
        .and_then(|v| v.to_str().ok())
        .and_then(|s| s.parse::<u64>().ok())
    {
        return reset.saturating_sub(now_unix).max(1);
    }
    60
}

/// Detect GitHub's primary rate-limit signals: either a 429 status, or
/// a 403 with an `X-RateLimit-Remaining: 0` header.
fn is_rate_limited_status(status: u16, headers: &ureq::http::HeaderMap) -> bool {
    if status == 429 {
        return true;
    }
    if status == 403 {
        let remaining = headers
            .get("X-RateLimit-Remaining")
            .and_then(|v| v.to_str().ok())
            .and_then(|s| s.parse::<u64>().ok());
        return remaining == Some(0);
    }
    false
}

fn retry_wait_seconds(resp: &ureq::http::Response<ureq::Body>) -> u64 {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    retry_wait_seconds_from_headers(resp.headers(), now)
}

fn is_rate_limited(resp: &ureq::http::Response<ureq::Body>) -> bool {
    is_rate_limited_status(resp.status().as_u16(), resp.headers())
}

/// True when the status code indicates a transient server-side failure
/// that's worth retrying with backoff (e.g. 502/503/504 during GitHub
/// flaps). 500 is less clearly transient but retrying once or twice
/// still costs little.
fn is_transient_server_error(status: u16) -> bool {
    matches!(status, 500 | 502 | 503 | 504)
}

/// Backoff seconds for the `attempt`-th retry of a transient server
/// error: 1, 2, 4, 8, ... capped at `MAX_RETRY_WAIT_SECS`. Exported
/// only to the unit-test module.
fn backoff_seconds(attempt: u32) -> u64 {
    let base = if attempt >= 63 {
        u64::MAX
    } else {
        1u64 << attempt
    };
    base.min(MAX_RETRY_WAIT_SECS)
}

/// Classify a `ureq::Error` as a transient network failure that's
/// worth retrying. Connection drops, hostname resolution failures,
/// timeouts, and IO errors all qualify. Structural errors (bad URI,
/// HTTP-level decode failures, body-size violations, TLS cert errors)
/// do not — those won't get better on retry.
fn is_transient_network_error(e: &ureq::Error) -> bool {
    use ureq::Error::*;
    matches!(
        e,
        Io(_) | Timeout(_) | HostNotFound | ConnectionFailed,
    )
}

impl GithubClient for HttpClient {
    fn pr_for_commit(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<Option<PullRequestRef>> {
        let key = Self::cache_key(owner, repo, sha);
        if let Some(cached) = self.cache.lock().unwrap().get(&key).cloned() {
            return Ok(cached);
        }

        let url = format!(
            "https://api.github.com/repos/{owner}/{repo}/commits/{sha}/pulls",
        );

        // Retry loop: up to MAX_RETRIES attempts sharing a single
        // budget across rate-limit waits, transient 5xx backoff, and
        // transient network errors (connection reset / DNS / timeout).
        let mut attempt = 0u32;
        let mut resp = loop {
            let mut req = self
                .agent
                .get(&url)
                .header("Accept", "application/vnd.github+json")
                .header("User-Agent", "entropyx");
            if let Some(t) = &self.token {
                req = req.header("Authorization", &format!("Bearer {t}"));
            }
            let resp = match req.call() {
                Ok(r) => r,
                Err(e) if is_transient_network_error(&e) => {
                    if attempt >= MAX_RETRIES {
                        return Err(format!(
                            "github API: network error after {MAX_RETRIES} retries: {e}",
                        )
                        .into());
                    }
                    let wait = backoff_seconds(attempt);
                    eprintln!(
                        "entropyx-github: network error (attempt {}/{}); retrying in {}s: {e}",
                        attempt + 1,
                        MAX_RETRIES,
                        wait,
                    );
                    std::thread::sleep(Duration::from_secs(wait));
                    attempt += 1;
                    continue;
                }
                Err(e) => return Err(Box::new(e)),
            };
            let status = resp.status().as_u16();

            // Rate-limit: sleep per response headers, retry.
            if is_rate_limited(&resp) {
                if attempt >= MAX_RETRIES {
                    return Err(format!(
                        "github rate limit exceeded after {MAX_RETRIES} retries. \
                         Set GITHUB_TOKEN env var to raise the limit from 60/hr to 5000/hr.",
                    )
                    .into());
                }
                let wait = retry_wait_seconds(&resp).min(MAX_RETRY_WAIT_SECS);
                eprintln!(
                    "entropyx-github: rate limited (attempt {}/{}); waiting {}s",
                    attempt + 1,
                    MAX_RETRIES,
                    wait,
                );
                std::thread::sleep(Duration::from_secs(wait));
                attempt += 1;
                continue;
            }

            // Transient 5xx: sleep exponential, retry.
            if is_transient_server_error(status) {
                if attempt >= MAX_RETRIES {
                    return Err(format!(
                        "github API returned transient HTTP {status} after \
                         {MAX_RETRIES} retries",
                    )
                    .into());
                }
                let wait = backoff_seconds(attempt);
                eprintln!(
                    "entropyx-github: server error {} (attempt {}/{}); retrying in {}s",
                    status,
                    attempt + 1,
                    MAX_RETRIES,
                    wait,
                );
                std::thread::sleep(Duration::from_secs(wait));
                attempt += 1;
                continue;
            }

            break resp;
        };

        let status = resp.status();
        if !status.is_success() {
            return Err(format!("github API returned HTTP {status}").into());
        }

        let prs: Vec<serde_json::Value> = resp.body_mut().read_json()?;

        // Multiple PRs may reference a commit (e.g. cherry-picks). The
        // API returns them oldest-first; the first is typically the
        // introducing PR.
        let result = prs.first().map(|pr| PullRequestRef {
            number: pr["number"].as_u64().unwrap_or(0),
            title: pr["title"].as_str().unwrap_or("").to_string(),
            state: pr["state"].as_str().unwrap_or("").to_string(),
            merged: pr["merged_at"].as_str().is_some(),
            merged_at: pr["merged_at"].as_str().map(String::from),
            author: pr["user"]["login"].as_str().map(String::from),
        });

        self.cache.lock().unwrap().insert(key, result.clone());
        Ok(result)
    }
}

/// In-memory client for unit tests. Pre-populate with `with_pr`, then
/// use as if it were an `HttpClient`.
pub struct MockClient {
    map: HashMap<(String, String, String), Option<PullRequestRef>>,
}

impl Default for MockClient {
    fn default() -> Self {
        Self::new()
    }
}

impl MockClient {
    pub fn new() -> Self {
        Self {
            map: HashMap::new(),
        }
    }

    /// Register a canned response. Use `None` to simulate "no PR
    /// references this commit" (a legitimate forensic outcome).
    pub fn with_pr(
        mut self,
        owner: &str,
        repo: &str,
        sha: &str,
        pr: Option<PullRequestRef>,
    ) -> Self {
        self.map.insert(
            (owner.to_string(), repo.to_string(), sha.to_string()),
            pr,
        );
        self
    }
}

impl GithubClient for MockClient {
    fn pr_for_commit(
        &self,
        owner: &str,
        repo: &str,
        sha: &str,
    ) -> Result<Option<PullRequestRef>> {
        Ok(self
            .map
            .get(&(owner.to_string(), repo.to_string(), sha.to_string()))
            .cloned()
            .unwrap_or(None))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_pr() -> PullRequestRef {
        PullRequestRef {
            number: 42,
            title: "Add forensic instrument for codebase dynamics".to_string(),
            state: "closed".to_string(),
            merged: true,
            merged_at: Some("2026-04-01T12:00:00Z".to_string()),
            author: Some("alice".to_string()),
        }
    }

    #[test]
    fn mock_returns_registered_pr() {
        let client = MockClient::new().with_pr(
            "acme",
            "widgets",
            "abc123",
            Some(sample_pr()),
        );
        let got = client
            .pr_for_commit("acme", "widgets", "abc123")
            .expect("mock call");
        assert_eq!(got, Some(sample_pr()));
    }

    #[test]
    fn mock_returns_none_for_unregistered_sha() {
        let client = MockClient::new();
        let got = client
            .pr_for_commit("acme", "widgets", "unseen")
            .expect("mock call");
        assert_eq!(got, None);
    }

    #[test]
    fn mock_can_simulate_no_pr_for_commit() {
        // A commit legitimately has no associated PR (e.g. direct push).
        // Registering `None` explicitly is different from "never registered":
        // the former is a signal, the latter is "unknown" in real usage.
        let client = MockClient::new().with_pr(
            "acme",
            "widgets",
            "direct_push_sha",
            None,
        );
        let got = client
            .pr_for_commit("acme", "widgets", "direct_push_sha")
            .expect("mock call");
        assert_eq!(got, None);
    }

    #[test]
    fn http_client_accepts_token_or_none() {
        // Smoke-test the constructor shape; no actual network call.
        let _anon = HttpClient::new(None);
        let _authed = HttpClient::new(Some("ghp_example".to_string()));
    }

    #[test]
    fn http_client_from_env_reads_github_token() {
        // Two cases: set and unset. Scope the env var with a unique
        // prefix so parallel tests don't stomp each other — we read and
        // restore the existing value explicitly.
        let prior = std::env::var("GITHUB_TOKEN").ok();
        // SAFETY: set/remove_var is safe in single-threaded test access;
        // cargo test runs tests in parallel threads but env vars are
        // process-global — if another test races here we might see
        // transient failures. For v0.1 scaffold, accept that risk; the
        // other tests in this module don't touch GITHUB_TOKEN.
        unsafe {
            std::env::set_var("GITHUB_TOKEN", "ghp_test_token");
        }
        let c = HttpClient::from_env();
        assert_eq!(c.token.as_deref(), Some("ghp_test_token"));
        unsafe {
            std::env::remove_var("GITHUB_TOKEN");
        }
        let d = HttpClient::from_env();
        assert_eq!(d.token, None);
        // Restore.
        if let Some(v) = prior {
            unsafe {
                std::env::set_var("GITHUB_TOKEN", v);
            }
        }
    }

    fn hdr(pairs: &[(&'static str, &str)]) -> ureq::http::HeaderMap {
        let mut m = ureq::http::HeaderMap::new();
        for (k, v) in pairs {
            m.insert(
                ureq::http::HeaderName::from_static(k),
                v.parse().unwrap(),
            );
        }
        m
    }

    #[test]
    fn rate_limit_detected_on_429() {
        let h = hdr(&[]);
        assert!(is_rate_limited_status(429, &h));
    }

    #[test]
    fn rate_limit_detected_on_403_with_remaining_zero() {
        let h = hdr(&[("x-ratelimit-remaining", "0")]);
        assert!(is_rate_limited_status(403, &h));
    }

    #[test]
    fn rate_limit_not_detected_on_403_with_remaining_available() {
        let h = hdr(&[("x-ratelimit-remaining", "42")]);
        assert!(!is_rate_limited_status(403, &h));
    }

    #[test]
    fn rate_limit_not_detected_on_403_without_header() {
        // 403 without the rate-limit header is "other auth failure",
        // not rate-limiting. Do not retry those.
        let h = hdr(&[]);
        assert!(!is_rate_limited_status(403, &h));
    }

    #[test]
    fn rate_limit_not_detected_on_200() {
        let h = hdr(&[]);
        assert!(!is_rate_limited_status(200, &h));
    }

    #[test]
    fn retry_after_header_wins() {
        let h = hdr(&[("retry-after", "17"), ("x-ratelimit-reset", "9999999999")]);
        assert_eq!(retry_wait_seconds_from_headers(&h, 0), 17);
    }

    #[test]
    fn x_ratelimit_reset_used_as_fallback() {
        let h = hdr(&[("x-ratelimit-reset", "1000")]);
        assert_eq!(retry_wait_seconds_from_headers(&h, 600), 400);
    }

    #[test]
    fn x_ratelimit_reset_already_passed_returns_at_least_one() {
        let h = hdr(&[("x-ratelimit-reset", "500")]);
        assert_eq!(retry_wait_seconds_from_headers(&h, 1000), 1);
    }

    #[test]
    fn no_headers_defaults_to_sixty_seconds() {
        let h = hdr(&[]);
        assert_eq!(retry_wait_seconds_from_headers(&h, 0), 60);
    }

    #[test]
    fn transient_5xx_classification() {
        for status in [500, 502, 503, 504] {
            assert!(
                is_transient_server_error(status),
                "{status} should be classified as transient",
            );
        }
        // Other 5xx codes are not treated as retriable — we don't want
        // to loop on 501 Not Implemented or 505 HTTP Version Not Supported.
        for status in [501, 505, 507, 511] {
            assert!(
                !is_transient_server_error(status),
                "{status} should not be retried",
            );
        }
        // 4xx is never retried via this path.
        for status in [400, 401, 403, 404, 429] {
            assert!(!is_transient_server_error(status));
        }
    }

    #[test]
    fn backoff_progression() {
        assert_eq!(backoff_seconds(0), 1);
        assert_eq!(backoff_seconds(1), 2);
        assert_eq!(backoff_seconds(2), 4);
        assert_eq!(backoff_seconds(3), 8);
        // Caps at MAX_RETRY_WAIT_SECS regardless of attempt count.
        assert_eq!(backoff_seconds(20), MAX_RETRY_WAIT_SECS);
        assert_eq!(backoff_seconds(100), MAX_RETRY_WAIT_SECS);
    }

    #[test]
    fn transient_network_error_classification() {
        // Variants we DO retry on.
        assert!(is_transient_network_error(&ureq::Error::Io(
            std::io::Error::new(std::io::ErrorKind::ConnectionReset, "reset"),
        )));
        assert!(is_transient_network_error(&ureq::Error::HostNotFound));
        assert!(is_transient_network_error(&ureq::Error::ConnectionFailed));

        // Variants we do NOT retry on — structural / unrecoverable.
        assert!(!is_transient_network_error(&ureq::Error::BadUri(
            "not a uri".to_string()
        )));
        assert!(!is_transient_network_error(&ureq::Error::TooManyRedirects));
        assert!(!is_transient_network_error(&ureq::Error::BodyExceedsLimit(
            100
        )));
        assert!(!is_transient_network_error(&ureq::Error::StatusCode(404)));
    }

    #[test]
    fn pull_request_ref_round_trips_through_json() {
        let pr = sample_pr();
        let json = serde_json::to_string(&pr).expect("serialize");
        let back: PullRequestRef = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back, pr);
    }
}
