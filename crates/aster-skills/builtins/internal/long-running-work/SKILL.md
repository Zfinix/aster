---
name: long-running-work
description: Carrying complex coding and project work through commands that take minutes or hours: evals, benchmarks, full builds and test suites, migrations, servers, multi-round runs. Use when a command may run past a few minutes, timed out once, or when the task spans several rounds of run, read results, fix, run again.
---

# Long-running work

1. **Background anything long.** Evals, benchmarks, full builds and test
   suites, data jobs, servers, watchers: `run_command` with `background:true`.
   It returns a job id at once and the job runs up to 4 hours. A foreground
   command is killed at its timeout, and a timeout is not a reason to narrow
   the work when the work is long by nature.
2. **Load the skills before you start, not after it breaks.** Read the skills
   that cover the task (`build-triage`, `verify-before-done`, the repo's own)
   first, and lay the steps out with `update_plan` when there is more than
   one. Mark each step as it lands.
3. **Make the run leave evidence.** Write results to a file the run owns
   (`results/run3.json`, `eval.log`), not only to stdout: a job keeps just the
   tail of its output. Name each round so later rounds do not overwrite it.
4. **Work while it runs.** Start the job, then do the independent work: edit
   the next fix, update docs, review the diff, send read-only questions to
   background sub-agents. Do not edit files the running job reads.
5. **Wait, do not poll.** When nothing is left to do, `command_job`
   action:`wait` (up to 600 seconds per call) until it exits. Never call
   `output` in a loop, and never `sleep` in a shell to pass time.
6. **Tell the user where things stand before every long wait.** One or two
   plain sentences: what is running, what changed since the last round, what
   comes next. Example: "Round 3 is running. Since round 2 the system prompt
   is down from 91K to 44K characters, and all 727 tests pass."
7. **When it exits, read before you act.** The exit message carries the code
   and last output; read the result file for the real numbers. A non-zero exit
   goes through `build-triage`. Compare against the previous round and say
   what moved.
8. **Close the loop.** Fix what the round showed, run the next round the same
   way, and stop when the goal is met or a round shows no progress. Stop jobs
   you no longer need with action:`stop`; never leave one running unexplained
   at the end of the work.
