---
name: aster-skill-authoring
description: Author skills for aster (and Claude Code): SKILL.md format, frontmatter rules, name/description validation limits, folder layout, scaffolding with `aster skills init`, and local testing with `aster skills add -l`. Use when writing a new SKILL.md, fixing a skill that fails to load, or publishing a skills repo.
---

# Authoring skills

A skill is a directory containing a `SKILL.md`: YAML frontmatter with `name` and `description`, then a markdown body of instructions. The directory may bundle extra resources (scripts, references); the whole directory is copied on install.

## Format

```markdown
---
name: my-skill
description: One or two sentences saying what it does and when to use it.
---

# My skill

Instructions the agent follows when the skill triggers...
```

Validation rules enforced by aster's loader:

- `description` is required, non-empty, max 1024 characters.
- `name` is optional (falls back to the directory name) but when present must be lowercase letters, digits, and hyphens only, max 64 characters.
- The file must open with a `---` frontmatter fence.

## Writing a good description

The description is the trigger: agents read it to decide whether to load the skill. State what the skill covers AND when to use it, and name the concrete commands, files, or phrases that should trigger it. Vague descriptions never fire.

## Workflow

```sh
aster skills init my-skill      # scaffold my-skill/SKILL.md
# edit my-skill/SKILL.md
aster skills add ./my-skill -l  # validate: lists the skill if it parses, warns if not
aster skills add ./my-skill     # install into .aster/skills to try it
```

`add -l` against a local path is the fastest lint: a skill that doesn't appear in the listing failed frontmatter validation.

## After writing a skill

Install it right away so it is live for the next turn, without waiting to be asked:

```sh
aster skills add ./my-skill --all --yes --force      # user-global
aster skills add ./my-skill --all --yes --force -p   # this project only (.aster/skills)
```

Re-run the same command after every edit; `--force` replaces the installed copy, and `--all` is required when no terminal is attached (`--yes` alone is refused). Do not save a memory about the skill you wrote: the skill is the record, and a memory that repeats it is noise.

## Repo layout for publishing

Host multiple skills under a `skills/` directory, one subdirectory per skill:

```text
skills/
  my-first-skill/SKILL.md
  my-second-skill/SKILL.md
```

Discovery walks up to 6 levels deep and skips `.git`, `node_modules`, `target`, `dist`, `build`. Consumers then install with `aster skills add owner/repo` or `npx skills add owner/repo`. Skill names must be unique within a repo; on collision the first found wins.

## Body guidelines

- Write for an agent, not a human tutorial: imperative instructions, exact commands, real flag names.
- Keep it grounded: only document behavior you have verified against the tool's `--help` or source.
- Prefer short sections with runnable code blocks over prose.
