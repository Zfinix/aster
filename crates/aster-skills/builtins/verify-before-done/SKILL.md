---
name: verify-before-done
description: Proving a change works before reporting it done, and what to do when the check fails. Use after editing code, before writing the final answer, and whenever deciding which check proves which claim.
---

# Verify before done

1. **Every code edit gets a check after the last edit.** Typecheck or build at
   minimum (`cargo check`, `bunx tsc --noEmit`, `dart analyze`), tests when
   they exist. A check that ran before your last edit proves nothing.
2. **Verify behavior, not just compilation.** For anything that serves or
   renders, probe the running artifact:
   `curl -s localhost:8082/page | grep -c "expected heading"` and
   `curl -s -o /dev/null -w "%{http_code}" localhost:8082/page`.
   A build that passes can still render the wrong thing.
3. **Wait by polling, never by sleeping.**
   `for i in $(seq 1 40); do curl -s localhost:8082 >/dev/null && break; sleep 0.5; done`
4. **A red check means fix, not report.** Name the root cause in one sentence,
   fix it, re-run the same command. Never end the turn on a failing check
   without saying so in the first sentence.
5. **Calling a failure pre-existing needs evidence.** Show the same failure on
   untouched code, or cite what does pass ("31 tests pass; the analyzer
   errors are a version mismatch, present before my change").
6. **Re-verify the neighbors.** After the fix, re-run the checks that passed
   earlier for behavior next to your change. Breaking the sticky header while
   fixing the alignment is the classic complaint; replaying an earlier check
   catches it.
7. **Report with the verdict first and the evidence inline.** Write:
   "Fixed. 15/15 tests pass, page renders the new section." Never "should
   work". Either "verified by <command>" or "unverified because <reason>".
