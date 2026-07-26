---
name: aster-chat-sessions
description: Drive aster chat programmatically and manage sessions and memory: one-shot --print/--json answers, --messages-json for caller-owned history, --continue and --session persistence, --allow-edits gating, and the sessions/memory commands. Use when scripting aster chat, integrating it into an editor or agent, or inspecting saved transcripts.
---

# Chat, sessions, and memory

`aster chat` talks to the review agent with read/search tools over the repo. In a terminal it opens a TUI; from scripts and agents always use the one-shot forms.

## One-shot answers

```sh
aster chat -p "why is finding 2 critical?"       # plain text (default when piped)
aster chat --json "summarize the last review"    # {"reply", "edits", "usage"}
aster chat --no-tools -p "explain this error"    # plain LLM turn, no repo tools
```

`--model` overrides the model for the turn (else `ASTER_MODEL`, else aster.yaml).

## History and persistence

```sh
aster chat --continue -p "and the second one?"   # seed the most recent session's history
aster chat --session <ID> -p "..."               # persist this turn into a session by id
aster chat --messages-json msgs.json -p "..."    # caller-owned history (or `-` for stdin)
```

`--messages-json` takes a JSON array of `{"role","content"}` with roles `user` | `assistant` | `system`. Combined with `--session`, the session only records the turn (the caller owns history); `--session` alone also seeds the session's prior history.

## Edits

`--allow-edits` exposes an `edit_file` tool to the agent, gated by the `permissions` block in aster.yaml (deny-first globs, protected paths). Leave it off unless the task is explicitly to change files, and diff the working tree afterward.

## Inspecting sessions

```sh
aster sessions                 # list saved sessions for this repo
aster sessions show <id>       # full transcript
aster sessions --json          # machine-readable (also on `show`)
```

## Durable memory

```sh
aster memory                        # list stored memory
aster memory add "we use pnpm"      # append a fact to project memory
aster memory add --title deploy "…" # save a titled block
aster memory --json                 # machine-readable
```

Memory is per-repo and fed to future chats and reviews; store durable facts (conventions, constraints), not one-off context.
