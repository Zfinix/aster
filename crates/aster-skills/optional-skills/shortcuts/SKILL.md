---
name: shortcuts
description: Run Apple Shortcuts on this Mac: list the available shortcuts first, then run one by exact name with optional input. Use when the user wants to trigger a shortcut or asks what shortcuts exist.
---

# Shortcuts

1. **List before you run.** The shortcuts live on this Mac, not in the repo, so
   their names cannot be known without asking. Call the `list` tool first and
   read the names back; guessing a name burns a turn and fails.
2. **Run by exact name and pass input.** `run` takes the shortcut name plus an
   optional `input` string, which becomes the shortcut's input. Pass the value
   the user gave (a name, a path, a number) as `input` instead of expecting the
   shortcut to prompt for it.
3. **Runs happen in the background.** Shortcuts run through `Shortcuts Events`,
   so the Shortcuts app does not open or steal focus. A shortcut that shows its
   own dialog may still need the user to click it.
4. **Read the result, and explain empty output.** A shortcut that returns a
   value gives it back as `output`; one that acts without returning gives an
   empty `output`, which is success, not failure. If the run fails with
   "automation access" or "canceled", say so plainly and give the fix: grant
   Automation permission, or pass `input` instead of leaving a dialog to fill in.
