---
name: context-economy
description: Keeping the conversation small while working with big artifacts: diffs, logs, many files, bulk research. Use when output would be large, when many files need reading, or when a diff or log must be analyzed.
---

# Context economy

1. **Big outputs go to files, then get queried.**
   `git diff main > /tmp/change.diff` then `grep -n "fn " /tmp/change.diff`;
   `gh pr diff 12 > /tmp/pr12.diff`; a build log to `/tmp/build.log` then
   `grep -m5 error /tmp/build.log`. Never print thousands of lines to look
   at twenty.
2. **Summaries before bodies.** `git diff --stat` before any full diff,
   `wc -l` before a full read, `ls -la` before catting files. Decide what to
   open from the summary.
3. **Delegate bulk reading.** Reading ten files to answer one question fills
   the conversation with nine files of noise. Send the question to the
   `explorer` agent and get back only the conclusion.
4. **Ranged reads, always.** You almost never need a whole file: read the
   function, not the module. Ask for the specific line range the search hit
   pointed at.
5. **Do not re-read or re-run what is above.** A file read earlier and
   unchanged is still in the conversation; an identical command gives an
   identical answer. Scroll back instead.
6. **Minimal-output probes for external checks.** Checking that a URL exists
   needs a status code, not the page. Checking a fact in a doc needs one
   grep, not the document.
