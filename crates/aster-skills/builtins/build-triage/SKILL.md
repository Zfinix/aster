---
name: build-triage
description: Reading build, test, and compiler failures fast. Use when a build or test command fails, hangs, times out, or produces a wall of output.
---

# Build triage

1. **Fix the first error, not the last.** Everything after the first error is
   usually cascade. `cargo build 2>&1 | grep -m1 -B1 -A6 "^error"`.
2. **Exit code 0 can still be a failure.** The exit code of a pipeline belongs
   to its last stage: `cargo build 2>&1 | tail -3` reports tail's success.
   Trust the output text (`error[`, `FAILED`, `panicked`) over the code.
3. **Bound the output.** `| tail -20` for verdicts (test runners put the
   verdict last), `| head -30` for first errors, `--stat` for diffs. Never
   dump a full build log.
4. **Never escalate a timeout.** A command that timed out will time out again
   with a bigger budget. Kill leftovers (`pkill -f cargo`), then narrow: one
   package, one test, one file. `cargo check -p one-crate` beats a 10 minute
   workspace build.
5. **Label pipeline stages** so partial output says where it died:
   `echo "=== build ===" && cargo check 2>&1 | tail -3 && echo "=== test ===" && cargo test 2>&1 | tail -5`
6. **Filter noise to signal.** A hundred identical warnings hide one error:
   `2>&1 | grep -E "^(error|FAILED|thread)" | head -10`.
