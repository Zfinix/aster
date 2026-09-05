---
name: background-processes
description: Running servers, watchers, and long-lived processes without hanging the tool call. Use when starting a dev server, running anything in watch mode, or when a command would run until killed.
---

# Background processes

1. **Never run a server in the foreground of a tool call.** It runs until the
   timeout kills it and the round is wasted. Background it and detach:
   `bash -lc "nohup bun run dev > /tmp/dev.log 2>&1 & echo started"`.
2. **Logs go to a file, not the pipe.** A backgrounded process writing to the
   pipe keeps the call alive. Redirect to `/tmp/<name>.log`, then read the
   log with `tail -20 /tmp/<name>.log` in a later call.
3. **Wait by polling the port, never by sleeping.**
   `for i in $(seq 1 40); do curl -s localhost:3000 >/dev/null && echo UP && break; sleep 0.5; done`
4. **Verify it actually started.** After the poll, check the log tail for the
   startup line or errors: `tail -5 /tmp/dev.log`. A backgrounded crash looks
   identical to success until you look.
5. **Kill by pattern when done or when restarting.**
   `pkill -f "bun run dev" || true` before starting a second instance; two
   servers on one port is a classic wasted hour.
6. **One-shot beats long-lived when possible.** Prefer `bun run build` plus a
   static check over starting a dev server, and `cargo test` over
   `cargo watch`. Reach for a background server only when the task needs a
   live process.
