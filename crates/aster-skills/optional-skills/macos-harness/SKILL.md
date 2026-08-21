---
name: macos-harness
description: Control a whole Mac from one persistent Python session with screenshots, PID-targeted input, an animated virtual pointer, targeted Apple Accessibility, Apple Events, Browser Harness CDP, and filesystem access. Use for native, Electron, browser, dialog, file, or cross-app tasks without moving the physical cursor or forcing apps into the foreground.
---

# macOS Harness

## Setup

The skill ships with aster; the CLI it drives does not. Install it once with
`uv tool install macos-harness`, then run `macos-harness doctor` to check
permissions without prompting (`--request` only with user approval).

## Surface

The CLI entry points are `doctor`, `apps`, `see <app>`, `state <app>`,
`repl`, `skill`, `telemetry`. The real harness is the Python session: stdin
programs preload `mac`, `browser`, `Path`, and `subprocess`.

```bash
macos-harness <<'PY'
print(mac.see("Finder")["path"])
PY
```

Verified bindings (checked against the installed CLI):

- `mac.see(app)` -> dict with `path` (screenshot file), `app`, `bounds`,
  `focus.target_is_frontmost`. Captures without focusing the app.
- `mac.key` / `mac.type` / `mac.click` / `mac.drag` / `mac.scroll` /
  `mac.move`: PID-targeted input; the physical cursor stays untouched.
- `mac.ax` is an object: `.dump(app)` (dict: `app`, `nodes`, `text`,
  `windows`, `screenshot`), `.query`, `.get`, `.set`, `.perform`,
  `.actions`, `.at`.
- `mac.script(applescript)` for Apple Events; `mac.windows(app)`,
  `mac.list_apps()`, `mac.snapshot()`, `mac.get_app_state(app)` for
  discovery and state.
- `browser.connect(name)` / `browser.wait(name)` for CDP into the user's
  real logged-in browser. Chrome shows an `Allow remote debugging?` sheet
  on first connect; approve it once there.
- Plain `Path` and `subprocess` for everything else.

## Minimize round trips

- Bundle deterministic, reversible steps into one program, then verify once.
  Opening search, typing a query, and capturing the results is one burst,
  not three calls.
- Stop at a genuine decision boundary: ambiguous identity, new coordinates,
  an irreversible action, or unexpected state. Inspect once, then run the
  next burst.
- Do not screenshot merely to confirm that a known shortcut opened a text
  field before typing. Let the final screenshot verify the whole sequence.
- Poll exact AX or Apple Events state inside the same Python program when
  possible; do not make the model repeatedly ask whether a transition
  finished.
- Use the cheapest strong end-state check: one screenshot for visible state
  or one exact API/AX query for semantic state; both only when they prove
  different things.

## Use the small surface

Think in six verbs: `see`, `key`, `type`, `click`, `ax`, `script`.

```python
frame = mac.see("Spotify")
mac.key("cmd+k", app="Spotify")
mac.type("Alessia Cara", app="Spotify")
mac.click(640, 420, app="Spotify")

item = mac.ax.at(640, 420, app="Spotify")
mac.ax.perform(item["element_index"], "AXPress")

mac.script('tell application "Spotify" to play')
```

Use ordinary Python for local context and one-off logic. Do not add
app-specific helpers when a short program can resolve the task.

## Choose the lowest useful mode

1. When identity depends on local context (`my`, `friend`, or prior
   activity), inspect that context and correlate stable fields; a loose text
   hit is not enough.
2. Use `mac.script()` for a known exact, focus-safe app command.
3. Otherwise use `mac.see(app)` and vision.
4. Prefer a known keyboard route; use a verified coordinate for a visible,
   low-risk target.
5. Use targeted `mac.ax` only when semantic identity or state matters. Do
   not dump a full AX tree before trying the direct route.

After a failed verified burst, switch mode or stop. Never repair uncertainty
with repeated keys, clicks, deletion loops, or bulk input.

## Keep the invariants

- Input targets an already-running app PID and never requests activation or
  raise.
- A background target becoming frontmost raises `FocusChangedError`; never
  manipulate focus to restore it.
- `mac.click()` is raw PID-targeted input. It never guesses an AX action.
- The animated pointer is click-through and never moves the physical cursor.
- `mac.move()` moves only that pointer; it cannot produce native hover.
- Inactive apps may reject raw clicks. After one verified failure, switch
  mode.
- Never launch a closed app or use a custom URL scheme when focus is
  forbidden.
- Screenshot coordinates come from the latest `mac.see()` and preserve
  window bounds and Retina scaling.

Secondary primitives are `mac.move`, `drag`, `scroll`, `show_pointer`, and
`hide_pointer`. `mac.ax.query()` returns compact matches and bounds fallback
traversal; lower `max_nodes` for especially large apps.

## Browser and permissions

Use `browser` for DOM, tabs, network, downloads, and uploads. Do not
substitute AX for CDP inside a web page. While Browser Harness connects,
macOS Harness accepts Chrome's exact `Allow remote debugging?` sheet through
system-wide AX. It never activates Chrome or emits a mouse event.

Run `macos-harness doctor` to inspect permissions without prompting. Run
`macos-harness doctor --request` only with user approval. Accessibility,
screen recording, and event posting are global; Apple Events Automation is
per target.
