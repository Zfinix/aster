# Security Policy

## Supported versions

Aster is pre-1.0 and moves quickly. Security fixes are applied to the `main`
branch. Until a stable release line exists, please track `main`.

## Reporting a vulnerability

Please do not report security vulnerabilities through public GitHub issues,
pull requests, or discussions.

Instead, use GitHub's private vulnerability reporting: open the repository's
**Security** tab and choose **Report a vulnerability**. This creates a private
advisory visible only to the maintainers.

Please include:

- A description of the vulnerability and its impact.
- Steps to reproduce, or a proof of concept.
- The version or commit you tested against.

We will acknowledge your report, keep you updated on remediation, and credit
you in the advisory unless you prefer to remain anonymous.

## Scope

Aster runs locally and talks to a model provider you configure. Note that any
diff and retrieved source context are sent to that provider. Reports about how
Aster handles credentials, subprocess execution (analyzer binaries), and the
local symbol index are in scope. The behavior of third-party model providers is
not.
