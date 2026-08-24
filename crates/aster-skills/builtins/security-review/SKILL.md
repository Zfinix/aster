---
name: security-review
description: Reviewing a diff or feature for exploitable vulnerabilities before it ships. Use when asked to do a security review, when code touches auth, input handling, queries, files, URLs, crypto, or payments, and before merging changes that accept untrusted input.
---

# Security review

Review the diff as an attacker would. Only report issues **introduced or
exposed by the changed lines**; pre-existing problems get one summary line at
most, not findings.

## What to hunt, in order of impact

1. **Injection.** String-built SQL, shell commands with interpolated input,
   LDAP/ORM raw fragments, template injection. Parameterize or escape; name the
   exact concatenation site.
2. **Broken authorization.** Endpoints and handlers that authenticate but never
   check ownership or role (`user_id` from the request instead of the session).
   IDOR: any resource fetched by an ID the client controls without an ownership
   check.
3. **Unvalidated input reaching dangerous sinks.** Path traversal
   (`PathBuf::join` / `path.join` with user segments), open redirects, SSRF
   (user-supplied URLs fetched server-side), unsafe deserialization of
   untrusted bytes.
4. **Output escaping.** User data rendered into HTML, markdown, shell output,
   logs, or SQL identifiers without escaping; `innerHTML`,
   `dangerouslySetInnerHTML`, unescaped template variables.
5. **Secrets and credentials.** Hardcoded keys, tokens in URLs, secrets logged,
   credentials in error messages returned to clients.
6. **Crypto and session.** Weak hashes for passwords (anything unsalted or
   non-argon2/bcrypt/scrypt), `Math.random`/naive RNG for tokens, missing
   expiry or signature checks, comparisons of secrets with `==` instead of
   constant-time.
7. **Web plumbing.** Missing CSRF protection on state-changing routes, CORS
   widened to `*` with credentials, cookies without `HttpOnly`/`Secure`/
   `SameSite`, permissive file-upload type checks.
8. **AI/LLM integration.** Provider API keys shipped to the client bundle,
   no spend cap on model calls, untrusted content concatenated into prompts
   (prompt injection), model output rendered as HTML/markdown or passed to a
   tool without validation.

## Detection commands

Run these over the changed files first; each hit is a candidate to trace by
hand, not a finding on its own:

- Secrets: `(api[_-]?key|secret|token|password)\s*[:=]\s*["'][A-Za-z0-9]`
- Client-submitted prices or roles: `price|amount|role|is_admin` read from
  request body/params and used without server-side lookup.
- Unverified JWTs: `jwt\.decode\(` without a matching `verify`; auth enforced
  only in middleware rather than in the handler.
- Raw queries: `queryRawUnsafe|raw\(|format!\(.*SELECT|\$\{.*(?:WHERE|INSERT|UPDATE)`.
- Dangerous sinks: `exec|eval|innerHTML|dangerouslySetInnerHTML|join\(.*user`,
  user-supplied URLs passed to fetch/reqwest/http clients.
- Never trust the client: every price, user ID, role, subscription status, and
  rate-limit counter must be validated server-side. If it only exists in the
  browser bundle or request body, an attacker controls it.

## How to work

- Read the full function around each changed line, not just the hunk. A taint
  source above the diff or a sink below it is still your finding.
- Trace untrusted input from its entry point (route, CLI arg, env, request
  body) to every sink it reaches.
- For each candidate, try to write the concrete exploit input. If you cannot
  construct one, say so and downgrade the confidence, or drop it.
- Frameworks matter: check whether the framework already escapes, parameterizes,
  or guards before flagging. A finding that the framework neutralizes is noise.
  Known framework guards (React auto-escaping, Prisma parameterization, RLS
  policies, webhook signature verification) suppress the finding entirely.

## Verification

A finding is not done until the fix is confirmed:

- After fixing, re-run the matching detection command; it must come back clean
  for that site.
- For authz fixes, show the request that was rejected before and succeeds or
  fails correctly now (two users, one resource).
- For secrets, rotation is part of the fix: a removed key that was committed is
  still burned. Say so explicitly.

## Reporting

Lead with the count and worst severity. Each finding states:

- **What breaks**, the exact input or state that triggers it, and the affected
  route/function with `file:line`.
- **Severity**: critical (exploitable now, no auth), high (exploitable with an
  account), medium (needs a specific setup), low (defense-in-depth).
- **The fix** in one line: parameterize here, add the ownership check there.

No padding findings. If the diff is clean, say so plainly; a clean bill from a
real pass is worth more than a list of hypotheticals.