---
name: gpt-astra
description: Optimize agent instructions for GPT-6 Astra (Codex). Use when the user mentions Astra, Codex, GPT-6, or asks to audit/rewrite AGENTS.md, skills, or prompts for newer models.
---

# GPT-6 Astra optimization

Astra handles nuance, ambiguity, and judgment better than previous models. Instructions written for older models often over-specify, over-constrain, or burn context on things Astra does on its own.

## Core principles

1. **Remove what Astra already does.** It runs tests, checks its work, and reads what it needs without being told. Instructions that push these behaviors waste context and can cause over-testing.

2. **Loosen decision boundaries.** Astra takes boundaries seriously. Strong "always ask first" language written for older models will make Astra stop where you'd want it to continue. Replace with per-workflow permission: "The local test suite uses disposable fixtures. Run it, fix failures caused by your change, and rerun without asking."

3. **Define completion, not just the first step.** Astra can feel tentative about how far to take a task. If the work includes "implement, inspect the result, and fix what fails," say that in the request. A requirement to stop for review after the first implementation pulls Astra toward an early stopping point.

4. **Keep descriptions short.** Skill descriptions compete for context. Each one should be the minimum needed to decide when to load the skill. Long descriptions get truncated, and "pick me" energy leads to wrong skill selection.

5. **Use progressive disclosure.** The root SKILL.md is a router. Put detailed workflows in companion files so Astra only reads what applies to the current task.

## When to apply

- Writing or editing a SKILL.md for this repo
- Auditing AGENTS.md for over-specification
- Writing task prompts for Astra
- Reviewing a skill that fires too often or not enough

## Companion docs

- `AUDIT.md` — step-by-step workflow for auditing AGENTS.md and skills against Astra's capabilities. Load this when the user asks for an audit.
