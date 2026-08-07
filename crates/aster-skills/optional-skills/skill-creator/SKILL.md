---
name: skill-creator
description: Turning a workflow this session learned into a reusable skill. Use when the user says save this as a skill or make a skill for this, when they repeat the same correction or instructions a second time, or when a multi-step procedure just worked and will clearly recur.
---

# Skill creator

1. **Check the index first.** If an existing skill already covers the
   workflow, extend that skill's file instead of creating a near-duplicate.
2. **Confirm before creating.** A skill is a repo or config file; say what
   you are about to write and where, and get a yes. Exception: the user
   already asked for the skill in this turn.
3. **Pick the root by scope.** Project-specific workflow:
   `.aster/skills/<name>/SKILL.md` in the repo. Personal preference that
   applies everywhere: `<aster home>/skills/<name>/SKILL.md`. Kebab-case
   name, under 64 characters.
4. **The description is the trigger, so write it as one.** The index shows
   only name and description; the body loads when a request matches. State
   what the skill does, then the concrete situations that should fire it:
   "Use when the user asks to X, mentions Y, or a Z fails."
5. **Body format: numbered imperative rules with counter-examples.** Each
   rule is one behavior, stated as a command, with a literal do-this and
   never-this where it helps. Write "Stage named files: `git add src/a.rs`.
   Never `git add -A`." Never write essays about philosophy.
6. **Capture the evidence, not the theory.** Put in the exact commands that
   worked this session, the failure that motivated the rule, and the check
   that proves the workflow succeeded. A skill mined from a real session
   beats one written from imagination.
7. **Keep it small.** A few hundred to a couple thousand words. Past that,
   split into two skills with distinct triggers.
8. **Frontmatter is exactly `name` and `description`** between `---` fences,
   then the markdown body. Verify after writing: the skill should appear in
   the index next session, and `read_skill` should return the body now.
9. **Corrections are skill fuel.** When the user corrects the same thing a
   second time, that is the signal: offer to fold it into a skill or a
   `remember` note so they never repeat it again.
