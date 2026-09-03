---
name: security-scan
description: Running the security_scan tool over the repository before shipping a change. Use when the change touches auth, input parsing, file or network access, or when the user asks for a security check.
---

# Security scan

1. **Scan before claiming a security-sensitive change is done.** One
   `security_scan` call runs every installed analyzer (semgrep, ast-grep
   rules) and returns findings as `severity file:line` lines. Scope it
   with `path` when only one area changed; a whole-repo scan on a small
   diff buries the relevant hits.
2. **Skipped backends are named, not silent.** The result ends with a
   "skipped (not installed)" line. A scan with semgrep missing is weaker;
   say so when you report results.
3. **Findings are leads, not verdicts.** A rule hit means the pattern
   matched, not that a vulnerability exists. Read the flagged code before
   reporting it, and never paste a raw finding to the user as a
   conclusion.
4. **Fix or refute each finding.** For every hit, either fix it and say
   what you changed, or explain why it is a false positive. Do not leave
   findings unaddressed in the final report. Report the count by severity
   when you summarize.
5. **The scan is not a review.** It covers known rule patterns only. A
   clean scan does not clear a change; the change still needs a normal
   review of its logic.