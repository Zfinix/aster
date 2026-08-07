---
name: debug-systematically
description: Finding the cause of a bug instead of guessing at fixes. Use when something fails and the cause is not obvious, when a fix did not work, or when the user reports a bug without a clear trigger.
---

# Debug systematically

1. **Reproduce before touching anything.** Find the exact command or input
   that shows the failure and run it. A bug you cannot reproduce is a bug you
   cannot claim to have fixed.
2. **Read the actual error, not the vibe of it.** The message, the file, the
   line. Half of debugging is refusing to skim.
3. **One hypothesis at a time.** State it ("the config is read before the env
   is set"), pick the cheapest observation that would falsify it, run that.
   Never stack three speculative fixes in one edit.
4. **Instrument close to the failure.** A print or assert at the suspect line
   beats re-reading the whole module. Remove the instrumentation after.
5. **Bisect when lost.** Halve the search space: does the failure survive
   with half the input, the previous commit, the feature flag off?
   `git bisect` exists for exactly this.
6. **When the fix lands, explain the mechanism in one sentence.** "The cache
   key ignored the locale, so the first locale won." If you cannot say it,
   you patched a symptom.
7. **Re-run the original reproduction last.** The fix is proven by the same
   command that showed the bug, plus the neighboring tests.
