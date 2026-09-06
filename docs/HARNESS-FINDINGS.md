# Harness findings

Open problems found by grading recorded sessions (`aster-eval`) and mining the
raw transcripts. Each entry states the evidence, why it costs rounds, and the
shape of a fix. Fixed items move to the changelog and, where a live eval can
hold the line, to a case in `crates/aster-eval/src/live.rs`.

Evidence window: 5 days of sessions under `~/.local/share/aster/sessions`,
586 tool calls.

## 1. `explore` silently loses its later steps

Ten `explore` results came back at 23,976–24,016 chars, pinned against
`MAX_TOOL_RESULT_CHARS` (24,000). Results are truncated once, whole, at the
dispatch site, so the steps that ran last are cut off entirely.

`agent` already solves this: it divides the budget by the number of reports and
caps each one. `explore` concatenates raw and takes the single cut.

This is the worst finding here, because it punishes exactly the behaviour the
system prompt spends twelve lines asking for. A model that batches four lookups
can lose the fourth, then re-read it in another round: strictly worse than not
batching. It is a plausible contributor to the 2% `explore` adoption rate.

**Fix:** give `explore` the per-step budget `agent` has, and label a clipped
step so the model knows to ask for the rest rather than assume it is empty.

## 2. `find_files` can never match a directory

`find` filters to `entry.file_type().is_file()`, so a directory pattern always
returns `no files matched`. Observed:

```
{"dir": "docs/06-concepts", "pattern": "04-authentication"}   -> no files matched
```

The directory existed. The model then guesses again, which is the same dead end
the `.gitignore` bug produced: a wrong-looking answer that reads as "not here"
instead of "wrong query shape".

**Fix:** match directories too and mark them with a trailing `/`, the way
`list_files` already renders them.

## 3. `run_command` argument-shape mistakes are silent

Two recurring shapes, both answered with a raw shell error rather than a hint:

```
{"command": "git",   "args": ["-lc", "git log --oneline -15 -- ..."]}
        -> unknown option: -lc
{"command": "cargo", "args": ["cargo check"]}
        -> error: no such command: `cargo check`
```

The first is `bash -lc` habit applied to the wrong binary. The second packs a
whole command line into one argument. Both are recoverable by inspection: `-lc`
as the first arg of a non-shell binary is always a mistake, and an argument
containing a space that names the command again is always a mistake.

39 of 308 `run_command` calls ended in an error or exit 1.

**Fix:** detect both shapes before spawning and return the corrected invocation
as the error text, so the retry is one round instead of several.

**Status:** partly shipped. A full argv sent as a list in `command`, and a
shell line sent as `command`, are both repaired before spawning
([chat.rs](../crates/aster-cli/src/chat.rs), `command_argv`). The two shapes
above, where the mistake is inside `args`, still pass through unrepaired.

## 4. Models invent absolute paths instead of using the working directory

```
cd /Users/me/repos/aster && git log --oneline -3   -> No such file or directory
find /Users/mdev/Downloads -name building-aster.pdf
```

The first invents a repo location; the second invents a username. Every call
already runs in the repo root, so the `cd` is unnecessary as well as wrong.

**Fix:** state the working directory and home once in the environment snapshot,
and have a failed `cd` to a non-existent absolute path answer with the real
`cwd` rather than the bare shell error.

## 5. `search_files` is used to locate files by path

```
{"query": "firebase/02-customizations", "dir": "docs"}
{"query": "04-authentication", "dir": "sidebars.js"}
```

A query that looks like a path fragment is a `find_files` question aimed at the
content search. It returns `no matches`, which reads as "absent" rather than
"wrong tool", and the model tries several more phrasings. This is most of the
28% `search_files` barren rate, which has not moved since the `.gitignore` fix
(that fix only helps when the path was ignored, not when the query was wrong).

**Fix:** when a barren query contains `/` and no regex metacharacters, run the
`find_files` interpretation and return those hits, labelled.

## 6. Repeat suppression does not cover the tools that repeat most

In the same window: `run_command` 35 duplicate `(tool, args)` pairs,
`read_file` 23, `search_files` 1. Only 2 dedupe pointers were returned.

`search_files`/`find_files`/`list_files`/`explore` are deduped, and `read_file`
has its own mtime cache, so the covered tools are already quiet. The uncovered
one is `run_command`, and it is not safely dedupable in general: re-running a
build or a test is legitimate.

**Fix:** narrow it to read-only commands. A repeated `git status`, `git log`,
or `ls` inside one turn can answer with a pointer; anything that writes or
builds cannot.

## 7. Same-tool streaks are the flailing signature worth alerting on

Runs of three or more consecutive calls to the same tool: `run_command` 217,
`read_file` 28, `search_files` 20, `explore` 10.

`run_command` dominates because commands are inherently serial, so the raw
count is not itself a defect. The useful signal is a streak where the arguments
barely change, which is what the ten-identical-searches case looked like.

**Fix:** count near-identical consecutive calls in `aster-eval` and report it
beside the barren rate, so the pathology is visible without mining transcripts
by hand.

## 8. The session store moved, which breaks historical comparison

Sessions now resolve through `default_home()` to `$XDG_DATA_HOME/aster` (or
`~/.local/share/aster`). Older sessions are still under `~/.aster/sessions`, and
`aster-eval` reads only the new location, so a report run today and one run last
week describe different pools.

**Fix:** an operations note rather than a code change. Pass the old directory
explicitly when comparing against history, and prefer `--json` baselines over
re-running against a pool whose composition shifts. The store now migrates
`~/.aster` into the data home on first use, so the two pools converge on
machines that have run a current build.

## 9. Long prose arguments degenerate, and the guard is not watching them

The highest-cost failure observed. Seven consecutive `run_command` calls in one
session died with `error: tool arguments were not valid JSON`, all trying to
post a markdown PR review comment of roughly 1,000 characters.

The cause is not escaping. Two repairs were tested against the six recorded
payloads and fixed none of them: escaping literal control characters inside
strings repaired zero. The tails show why:

```
...steering readers away from the deprecated APIm the deprecated API.\""}
...`authenticationKeyManager` — treat the legacy property the same way.\n\\\"\"]"}
```

`deprecated API` appears twice, spliced as `APIm the`. That is text
degeneration happening *inside the tool-call argument stream*, which corrupts
the JSON structure. The payload is not mis-escaped, it is damaged.

`RepetitionGuard` exists for exactly this and works: in the streaming loop it is
fed `choice.delta.content` and aborts the turn when output degenerates. But the
`choice.delta.tool_calls` branch below it only appends
`function.arguments` to a buffer. Nothing watches that stream. So degeneration
in prose is caught, and degeneration in an argument is not, surfacing several
frames later as a parse error with no hint of the real cause.

The retry then makes it worse: the parse error goes back as tool output, the
model re-emits the same long body, and degenerates again. Seven times, until the
user gave up and swore at it.

**Fix:** feed the guard on argument fragments as well as content, so a damaged
argument aborts and retries as degeneration rather than arriving as invalid
JSON. Separately, a second parse failure on the same tool in one turn should
prompt for a shorter body or a file-based path rather than replaying the error.

**Not the fix:** a JSON repair library. Eight exist on crates.io
(`json-repair`, `jsonrepair-rs`, `json_repair_rs`, `safe-json-repair`, and
more), all ports of the JS `jsonrepair` idea. They fix malformed-but-intact
JSON: trailing commas, single quotes, unquoted keys, missing brackets. None can
recover text the model corrupted, and guessing at damaged content is worse than
failing, because a repaired-but-wrong `--body` posts the wrong comment to a
real pull request.

## 10. A rejected command shape is retried unchanged

`gh pr diff 732 -- <path>` was issued six times across one session and failed
identically every time:

```
stderr: accepts at most 1 arg(s), received 2
```

Nothing in the loop notices that this exact invocation already failed. The same
pattern appears with `gh pr review --json` (`unknown flag`) and a hyphenated
repo name (`serverpod/serverpod-docs`, 404) that was eventually corrected by
guessing.

This is the user-visible flailing that prompted, verbatim: *"You repeated the
same tool calls with the same results three times in a row. Stop repeating."*

Finding 6's dedupe does not catch it, because dedupe keys on exact arguments and
suppresses the *result*; it never tells the model the shape itself is rejected.

**Fix:** when a command fails and an identical earlier call in the same turn
failed the same way, answer with that fact instead of replaying the stderr, so
the retry has to change something.

## 11. The same remote document is refetched a dozen times

One session ran `curl -sL .../pull/732.diff` piped into `head`, `tail`, `sed`,
`grep`, and `wc -l` at least ten times, re-downloading a 995-line diff on each.
It eventually saved it to `/tmp/pr732.diff` and read ranges from there, which is
what it should have done first.

**Fix:** guidance rather than code. The command guidance already says to bound
noisy output; it should also say to fetch a remote document once to a file and
read ranges from it.

## Still open from the first pass

Batching has not moved: batch factor 1.30 at first measurement, 1.36 now, with
roughly 80% of rounds carrying a single call. `explore` is used 34 times against
586 calls. Prompting has been tried across every model and ignored.

The candidate fix is mechanical: make `explore` the only lookup tool and let a
one-step call be the degenerate case, so batching is structural rather than
requested. Finding 1 should land first, since a batching tool that drops data
is an argument against batching.

`aster-eval live` can now measure whether that change helps, but its deltas need
repeats (median of N) before a difference between models or builds is
trustworthy. Single runs of one model varied 1, 1, 3, 2 on the same case.
