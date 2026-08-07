---
name: write-tests
description: Writing tests that catch regressions instead of restating the code. Use when adding tests, when a fix needs a regression test, or when the user asks for coverage.
---

# Write tests

1. **Copy the house style first.** Open the nearest existing test file and
   match its layout, naming, helpers, and assertion style. A test that looks
   foreign gets rewritten in review.
2. **Name tests as behaviors, hierarchically.** `timeout_keeps_partial_output`,
   `edit_mismatch_embeds_closest_region`: the name states the guarantee, and
   a shared prefix groups the family for filtered runs.
3. **Test the behavior, not the implementation.** Assert on outputs and
   effects, not on private state or call counts. A test that breaks on every
   refactor protects nothing.
4. **Every bug fix ships its regression test.** Write the test first, watch
   it fail for the right reason, then fix. A test that never failed proves
   nothing.
5. **One behavior per test.** Three asserts about one outcome is fine; three
   scenarios in one test means the name lies about two of them.
6. **Cover the edge that motivated the code.** Empty input, the boundary
   value, the error path. The happy path usually already works.
7. **Run the new tests plus the file's old ones,** and quote the counts in
   your report.
