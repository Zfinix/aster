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

Start here:

- **aster-capabilities** - the whole map: every command, flag, slash command, key, and config block, and which one to reach for.
- **aster-cli** - driving the `aster` CLI day to day: chat, reviews, fixes, sessions, memory, skills.

Automating it:

- **aster-review-ci** - reviews with no terminal attached: `--pr`, `--json`, `--stream`, `--comment`, GitHub Actions.
- **aster-fix-workflow** - piping `review --json` into `aster fix`: dry-run first, permission gating, curating findings.
- **aster-chat-sessions** - scripting `aster chat` (`--print`, `--json`, `--messages-json`), sessions, and durable memory.
- **aster-config** - aster.yaml: models per stage, analyzers, globs, and the permissions block.

Extending it:

- **aster-skill-authoring** - writing and publishing skills: SKILL.md format, validation limits, local testing.
- **aster-planning** - breaking a multi-file task into a plan and working through it.
- **aster-ui-demo** - demoing UI work in the real webview and dev server rather than a throwaway mockup.
- **gpt-astra** - rewriting AGENTS.md, skills, and prompts for GPT-6 Astra (Codex).
