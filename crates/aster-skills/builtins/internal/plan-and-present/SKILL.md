---
name: plan-and-present
description: Researching, drafting, and presenting a plan the user can actually judge. Use when the user asks to plan first, when the work is large or irreversible enough that the approach should be agreed before any edit, and whenever you are about to call write_plan or exit_plan_mode.
---

# Plan and present

1. **Read before you write a word of it.** Open every file the plan will
   change, not just their names, and trace who calls the functions you will
   touch. When the plan picks a library or leans on one, read the parts of its
   source you depend on. A plan written from a guess about the code is the
   expensive kind of wrong: the user approves it, and the first edit finds out.
   "`main.rs` or wherever the commands live" in a plan means you stopped
   reading too early. Stay read-only until they answer.
2. **The plan is a file you build up, not a message you compose at the end.**
   Write a skeleton with `write_plan` as soon as you know the shape of the
   work, then fill each section in as you read. When a finding changes your
   mind, edit that section with `old_str`/`new_str`. The draft is where you
   think; the approval is where the user reads the result.
3. **Settle the forks before you write them down.** A choice that is genuinely
   the user's (scope, a trade-off with no right answer in the code) goes to
   `ask_user` first, and the plan records what they chose. Do not leave the
   decisions as a list of questions at the bottom.
4. **Write these sections, in this order:**
   - `## Context`: what is true today and why the change is needed. Every
     claim about the code cites `path:line`. This is how the user checks that
     you are aimed at the right problem.
   - `## Decisions`: each choice, the evidence behind it, and the alternative
     you rejected with a real reason. When reading the code reversed your first
     idea, say so and show what reversed it. That is the most useful paragraph
     in a plan.
   - `## Changes`: per file, the functions that change and what changes in
     them, plus the existing code you will reuse instead of writing new.
     Specific enough that someone else could carry it out.
   - `## Risks`: migrations, deletions, rewrites, published artifacts,
     anything touching credentials, and what you are still unsure of. If
     something is irreversible, it also goes in the first line of Context.
   - `## Verification`: the exact commands to run and the result each should
     give. "Run the tests" is not verification; `cargo test -p aster-cron
     schedule` with the cases it must cover is.
5. **Scale it to the work, never below the evidence.** A subsystem earns a
   long document. A three-file fix earns short sections. Short is fine; vague
   is not. Every section still names real files and real commands.
6. **Name the decisions, not the steps.** "Split the device-code flow out of
   `provider.rs:212` before touching the token store, because both backends
   call it" is a decision. "Implement OAuth" is a step, and a step is what the
   user already asked for.
7. **`update_plan` is the progress strip, not the plan.** Its steps track what
   is done while you work. The document wins the approval, then the approved
   work becomes `update_plan` steps, kept current through completion.
8. **Present once, then stop.** Call `exit_plan_mode` and wait. No edits, no
   state-changing commands, no "starting on this while you read". The user's
   answer is the point of asking.
9. **A rejection is information, not a retry.** Edit the sections they
   objected to and present again. Do not re-send the same plan with the
   objection unaddressed, and do not narrow the plan just to get a yes.
