---
name: aster-ui-demo
description: How to demo UI work the way Claude does it, never with throwaway HTML mockups. Build the change in the real webview, verify it in the browser harness, open the live dev server for the user to click through, and run the real test suites. Use when the user asks for a demo, a preview, or "let me see" any UI change in the desktop or VS Code surfaces.
---

# Demo UI work in the real surface

Never build a standalone HTML mockup to show a UI idea. Users judge UI by
clicking the real product, and a mockup cannot answer the only questions that
matter: does it work, does it match the surrounding design, does it survive
real data.

## The workflow

1. **Build it for real.** Implement the change in the actual frontend source,
   even when the user only asked to "see it first". For this repo that means
   `editors/vscode/webview/` (shared webview used by both the VS Code panel and
   the browser harness) or `desktop/` for Tauri surfaces.
2. **Serve it in the browser harness.** From `editors/vscode/`, run
   `bun run dev:web` (builds the webview, then runs `out/devhost/main.js`, a
   real browser-runnable host with a stubbed `vscode` API). Background it and
   note the port (default 4327).
3. **Click through it yourself first.** Use browser automation to exercise the
   feature before the user sees it: open the panel, trigger the feature, resize
   narrow (300px) to check layout. Fix what breaks.
4. **Open it for the user.** Use `open_preview` on the harness URL so they can
   click through the running app. State plainly that it is the real surface,
   running in a browser host.
5. **Run the real checks** before calling it done: `bun run test` (vitest) in
   `editors/vscode/`, and `make check` or scoped `cargo test -p aster-cli` for
   Rust changes.

## Reporting

Reference real files with line numbers, describe behavior the user can trigger
by clicking, and note anything the browser harness cannot reproduce (native
menus, real extension API calls) so the user knows what still needs testing in
the actual editor.

## Why not mockups

A mockup gets one reaction ("looks fine") and then the work is thrown away.
The real build gets the same reaction plus every real bug found before the
feature ships. The mockup path only wastes the turn it saves.
