# Security policy

## Scope

entropyx is a local-first CLI that reads a git repository and emits JSON.
The security surface is small but non-zero:

- **`entropyx scan` reads arbitrary bytes** from blobs, commit metadata,
  and author lines. A malformed or adversarial repository should not
  produce a crash, infinite loop, unbounded memory growth, or code
  execution.
- **`entropyx --github` makes HTTPS requests** to `api.github.com`
  under an opt-in flag. The request path, headers, and token handling
  should not leak credentials or accept injected URLs.
- **`entropyx explain` resolves handles** against blob SHAs and commit
  SHAs. A maliciously crafted handle string should be rejected, not
  consulted as a path.
- **Disk caches** live under `$ENTROPYX_CACHE_DIR` /
  `$XDG_CACHE_HOME/entropyx/`. They store parsed public-API items and
  PR metadata — no secrets.

## What we consider a vulnerability

- Any crash, panic, or denial-of-service reachable by feeding the tool
  a crafted repository or network response.
- Any read or write outside the repository path passed on the command
  line (path traversal in rename resolution, symlink abuse, etc.).
- Any logic that causes credentials (GitHub token, env vars) to appear
  in stdout, stderr, the cache files, or error messages.
- Any bypass of the determinism guarantee (RFC-001) that could be used
  to influence downstream trust.

## Reporting

**Please do not open a public GitHub issue for security-sensitive
reports.** Instead email `don@codetestcode.io` with:

1. A description of the issue.
2. A reproduction: the smallest repository / input that triggers it,
   or a minimal test case if you've already localized the cause.
3. Your preferred handle for credit (or "prefer anonymous").

You'll get an acknowledgment within 72 hours. A fix, advisory, and
credit line in the CHANGELOG will follow as fast as the severity
warrants.

## What we won't fix

- Issues that require a trusted user to willfully run
  `entropyx --github <malicious-slug>` against a repository they have
  network access to attack. We don't defend against the operator
  pointing the tool at their own target.
- Heavy macro'd C/C++ files producing low `S_n` is a correctness
  limitation, not a security issue.

## Supported versions

Only the latest released version gets security fixes. v0.1.x fixes
are backported for a 90-day window after v0.2 ships.
