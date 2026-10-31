---
name: design-canvas
description: Building and reviewing visual work as an artifact or a design canvas. Use when the user asks to design, mock up, compare, or review UI, and when a thread of CSS patches needs one page the user can actually look at. An artifact is one self-contained HTML page; a canvas is several artboards laid out together with a generated viewer.
---

# Artifacts and design canvases

Two tools, one job: give the user something to look at instead of a description.

- **`artifact`** for one page: a report, a visual explainer, a mockup, a one-pager.
- **`canvas`** for several boards: screens of a flow, variants of one screen, before and after.

Both write under `.aster/` in the repo and open in the browser with `open_preview`.

## Artifact

1. Build the complete document first. One `<style>` block, no build step, no external framework unless the project already has one.
2. Write it with `artifact` action `create` (or `update` for an existing name), passing `name`, `title`, and the whole `html`.
3. Open it with action `open` before ending the turn. Once per page per turn.

```json
{"action":"create","name":"pricing-page","title":"Pricing page","html":"<!doctype html>…"}
```

Pages live at `.aster/artifacts/<name>/index.html`. `list` shows what exists.

## Canvas

Use a canvas when one page cannot carry the comparison. Each board is its own HTML document; the viewer lays them in rows with their titles.

1. `{"action":"create","name":"chat-redesign","title":"Chat redesign"}` once.
2. `{"action":"board","name":"chat-redesign","board":"main","title":"Chat panel","width":680,"height":1400,"html":"<!doctype html>…"}` per board. Re-sending a board replaces it in place.
3. `{"action":"open","name":"chat-redesign"}` at the end.

Boards live at `.aster/canvases/<name>/project/<board>.html`, the index at `canvas.json`, and the viewer at `viewer.html`. Keep boards self-contained: a board cannot see another board's CSS or scripts.

## Designing well

- **Adopt what exists.** Find the project's tokens, theme, and components before inventing a palette. A token file outranks every default below.
- **One accent, three weights.** A ground, a foreground, a dim foreground, one accent, one danger colour. Do not tint every surface.
- **Type does the hierarchy.** Two sizes and two weights go further than five colours. Body text at 4.5:1 contrast, even the 11px labels.
- **Space in one rhythm.** Pick a scale (4, 8, 16, 24) and use it everywhere. Rows in a list share one gap; blocks share a larger one.
- **Real content.** No lorem ipsum, no invented metrics. Use the user's own words and files; a missing fact becomes a visible placeholder like `[PRICE]`.
- **No AI tropes.** No gradient washes, no left-border cards, no emoji, no fake browser chrome. If the design needs a metaphor, draw it.
- **Accessible as drawn.** Real `<button>`, `<a href>`, `<input>` with a label. Icon-only buttons get `aria-label`. Decorative SVG gets `aria-hidden`.
- **Verify by looking.** Open the artifact or canvas and read it at phone width as well as desktop. A clipped or unreadable board is not done.

## Review loop

When the user asks for a design, build the boards first and open the canvas. Take feedback as edits to the boards, not as a new explanation. Keep the canvas name stable so the same viewer updates in place.
