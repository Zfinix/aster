# Auditing instructions for Astra

Run this audit when the user asks to clean up AGENTS.md, skills, or prompts for GPT-6 Astra. Work through each section, report findings, and propose specific cuts. Do not edit until the user approves.

## Phase 1: AGENTS.md audit

For each section in AGENTS.md, ask:

### Does Astra already do this?

- **Testing instructions.** "Run tests after changes," "verify your work," "check before reporting done." Astra does this on its own. Flag for removal unless the instruction adds a repo-specific constraint (which test runner, which flags).
- **Reading instructions.** "Read the file before editing," "understand the codebase first." Astra reads what it needs. Flag for removal unless it points to a specific doc that must be read.
- **Style rules.** Keep the ones that are repo-specific (no em dashes, Conventional Commits). Flag generic ones ("write clean code," "follow existing patterns") for removal.

### Is this boundary too tight?

- **"Ask before" rules.** For each one, ask: would the user actually want to be interrupted here? If the action is safe and reversible (running local tests, formatting, linting), replace with a permission grant.
- **Approval gates.** "Wait for approval before..." on steps that are always safe. Replace with: "The local test suite uses disposable fixtures and has no production access. Run it, fix failures caused by the requested change, and rerun affected tests without asking for approval at each step."

### Is this over-specified?

- **Elaborate workflows.** Multi-paragraph recipes for things Astra can figure out. Replace with the constraint that matters, not the step-by-step.
- **"Always do X before Y"** rules. If Astra can judge when X is needed, remove the rule.

### Report format

For each finding, give:
- **Section** and line range
- **Verdict**: keep / trim / remove / rewrite
- **Why**: one sentence
- **Proposed replacement** if rewriting

## Phase 2: Skill audit

For each skill in `skills/`:

### Description check

- Is it under ~200 characters? If not, trim it.
- Does it say when to use the skill, not just what it does? Add a trigger phrase if missing.
- Does it have "pick me" energy (overclaiming, too broad)? Narrow it.

### Body check

- Is the root SKILL.md a router or does it dump everything? If the skill has multiple workflows, split detailed instructions into companion files.
- Are there step-by-step recipes Astra doesn't need? Remove them, keep the constraint.
- Are there instructions for older models that over-constrain Astra? (e.g., "always read X before Y," "never do Z without asking")

### Report format

Same as Phase 1: skill name, verdict, why, proposed change.

## Phase 3: Summary

After both phases, give a one-paragraph summary:

- Total findings
- How many are cuts vs rewrites
- Estimated context savings (rough token count)
- The single highest-impact change
