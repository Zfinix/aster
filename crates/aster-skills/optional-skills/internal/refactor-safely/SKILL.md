---
name: refactor-safely
description: Renaming, moving, and restructuring code without breaking behavior. Use when the task is a rename, a file move, an extract, or any restructuring the user expects to be behavior-neutral.
---

# Refactor safely

1. **Map the blast radius first.** Search every reference before changing
   one: the definition, call sites, tests, docs, config strings,
   serialization. A rename that misses a string reference compiles and then
   breaks at runtime.
2. **Green between every step.** Refactor in compilable increments: move,
   check, rename, check. A ten-step refactor with one check at the end turns
   one mistake into an hour of archaeology.
3. **Never mix behavior change into a refactor.** If you spot a bug mid
   refactor, note it, finish the neutral change, then fix the bug as its own
   change. Mixed diffs are unreviewable and unbisectable.
4. **Let the compiler drive mechanical renames.** Change the definition
   first, then fix every error it reports; the error list is your checklist.
5. **Match the destination's conventions.** Moved code adopts the naming,
   error handling, and comment density of the file it lands in.
6. **Prove neutrality at the end.** The same tests that passed before must
   pass after, with no test edits beyond imports and paths. Any test that
   had to change semantically means it was not a refactor.
