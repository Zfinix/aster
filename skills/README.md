# Skills

Agent skills shipped with this repo. Each skill is a directory with a `SKILL.md` (frontmatter `name` + `description`, body with instructions), the same format used by Claude Code, `npx skills`, and `aster skills`.

## Installing

Claude Code / Cursor / other agents, via [skills.sh](https://skills.sh):

```sh
npx skills add Zfinix/aster
```

Claude Code, manually:

```sh
git clone https://github.com/Zfinix/aster /tmp/aster-skills
cp -R /tmp/aster-skills/skills/aster-cli ~/.claude/skills/aster-cli
```

Aster itself:

```sh
aster skills add Zfinix/aster
```

## Available skills

- **aster-cli** - how to drive the `aster` CLI: reviews, chat, fixes, sessions, memory, and skill management.
- **aster-review-ci** - running reviews non-interactively: `--pr`, `--json`, `--stream`, `--comment`, and GitHub Actions wiring.
- **aster-config** - aster.yaml reference: models per stage, analyzers, globs, and the permissions block.
- **aster-fix-workflow** - piping `review --json` into `aster fix`, dry-run first, permission gating, curating findings.
- **aster-chat-sessions** - scripting `aster chat` (`--print`, `--json`, `--messages-json`), sessions, and durable memory.
- **aster-skill-authoring** - writing and publishing skills: SKILL.md format, validation limits, local testing.
