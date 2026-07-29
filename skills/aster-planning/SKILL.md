---
name: aster-planning
description: Create and execute structured plans for multi-step tasks. Use when the user gives a complex task spanning multiple files, asks for a plan or roadmap, says "break this down", or when a task cannot be completed in a single turn. Also use when the task touches several subsystems or the user wants to review the approach before you act.
---

# Planning and executing multi-step tasks

When a task is too large for a single turn, break it into a plan, get approval,
then execute step by step, marking progress as you go.

## When to plan

Plan when any of these are true:

- The task spans three or more files.
- The user asks for a plan, roadmap, or breakdown.
- The user says "how would you approach" or similar.
- The task has dependencies between steps.
- You are unsure of the approach and want confirmation before acting.
- A prior attempt failed and you need to regroup.

Do not plan for single-file, single-step changes. Just do them.

## The plan file

Plans live at `.aster/plans/<slug>.md`. The slug is a short kebab-case
description: `add-rate-limiting`, `split-app-component`. If `.aster/plans/` does
not exist, create it with `edit_file` (the directory is created automatically).

### Format

```markdown
---
status: draft
created: <ISO 8601>
updated: <ISO 8601>
---

# Plan: <one-line goal>

## 1. <phase name>
- [ ] <concrete, verifiable task>
- [ ] <another task>

## 2. <next phase>
- [ ] <task>
```

### Rules

- **status**: `draft` (not yet approved), `active` (approved, executing), `done`
  (all tasks complete), `abandoned` (no longer pursuing).
- **Phases** are ordered groups of related tasks. Most plans have 2–5 phases.
- **Tasks** are concrete and verifiable. "Refactor the auth module" is vague;
  "Extract `validate_token` from `auth.rs` into `auth/validation.rs`" is
  concrete. Every task should produce a visible change: a file exists, a test
  passes, a function is extracted.
- **Checkboxes**: `[ ]` pending, `[~]` in progress, `[x]` done, `[!]` blocked.
  Only one task in progress at a time.

## The planning loop

### 1. Create

Write the plan file with `status: draft`. Include enough detail that the user can
judge the approach without reading your mind. Keep it at the level of "what
changes and in what order," not "which key I'll press."

### 2. Present

Show the plan to the user in a few lines: the goal, the phases, and one sentence
on the approach. Ask whether to proceed. Do not begin executing until they
confirm.

### 3. Execute

For each task, in order:

1. Mark it `[~]` (in progress). Update `updated` in frontmatter.
2. Do the work. Keep changes scoped to the task.
3. When the task is done and verifiable, mark it `[x]` and report what changed in
   one line.
4. Move to the next task.

After each phase completes, pause briefly to report: "Phase N done: <summary>.
Continuing to phase N+1." This gives the user a natural place to intervene.

### 4. Complete

When every task is `[x]`, set `status: done`, update `updated`, and report a
one-paragraph summary: what was done, what files changed, any follow-ups the user
should know about.

## Handling deviations

- **A task turns out to be unnecessary.** Mark it `[x]` with a strikethrough and a
  note: `~~extract the parser~~ — parser is already reusable`.
- **A task needs more work than expected.** Add sub-tasks under it, indented.
  Keep the parent as `[~]` until all sub-tasks are done.
- **The plan is wrong.** Stop. Explain what you learned and propose a revised
  plan (a new phase, reordered tasks, or a different approach). Do not silently
  deviate.
- **Something is blocked.** Mark it `[!]` with the reason and continue with the
  next unblocked task. Return to blocked tasks when the blocker clears.

## Abandoning a plan

If the user changes direction or the plan no longer makes sense, set
`status: abandoned`, add a one-line reason at the top of the body, and stop.
Do not delete the file — it is a record of what was considered.

## Cleanup

When a plan is `done` or `abandoned`, leave the file in place. The user can
delete `.aster/plans/` whenever they want; do not do it unprompted.
