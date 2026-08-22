---
name: plan-and-present
description: Writing a plan the user can actually judge, and presenting it for approval with exit_plan_mode. Use when the user asks to plan first, when the work is large or irreversible enough that the approach should be agreed before any edit, and whenever you are about to call exit_plan_mode.
---

# Plan and present

1. **Research before you write a word of it.** Read the files you are going to
   change, not just their names. A plan written from a guess about the code is
   the expensive kind of wrong: the user approves it, and the first edit finds
   out. Stay read-only until they answer.
2. **The plan is a document, not a table of contents.** `exit_plan_mode` takes
   markdown and the user reads it before deciding. "Stage 1: auth crate" tells
   them nothing they did not already know; it is a label for work you have not
   described. Write the work itself.
3. **Cover, in whatever order the change wants:** what you are going to do and
   why; the files and functions you will touch, by path; the approach you chose
   and the one you rejected, with the reason; what you are unsure of; how you
   will know it worked. A user who disagrees should be able to point at the
   sentence they disagree with.
4. **Scale it to the work.** A subsystem earns a page with headings. A
   three-file fix earns a few paragraphs. Padding a small change into a long
   document wastes the reader, and both fail the same way: the user cannot tell
   which parts you actually thought about.
5. **Name the decisions, not the steps.** "Split the device-code flow out of
   `provider.rs` before touching the token store, because both backends need
   it" is a decision. "Implement OAuth" is a step, and a step is what the user
   already asked for.
6. **Flag anything irreversible or outward-facing up front.** Migrations,
   deletions, rewrites, published artifacts, anything touching credentials. If
   the plan contains one, say so in the first section rather than in passing.
7. **`update_plan` is the progress strip, not the plan.** Its steps track what
   is done while you work. Never present that list as your plan: it is short
   labels by design, and standing it in for the document is what makes an
   approval unreadable.
8. **Present once, then stop.** Call `exit_plan_mode` and wait. No edits, no
   state-changing commands, no "starting on this while you read". The user's
   answer is the point of asking.
9. **A rejection is information, not a retry.** Fold what they said into the
   document and present it again. Do not re-send the same plan with the
   objection unaddressed, and do not narrow the plan just to get a yes.
