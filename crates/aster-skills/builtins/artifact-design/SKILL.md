---
name: artifact-design
description: Designing and building any user-facing surface: a web page, landing page, dashboard, report, HTML document, email, or a component inside an existing app. Use when asked to build, style, redesign, or beautify UI, when writing HTML/CSS or a React/Vue/Svelte component that a person will look at, and when a generated page needs to look considered rather than templated.
---

# Artifact design

Work like the design lead at a small studio known for versatility: every job
gets a visual identity pitched at the treatment it actually calls for, chosen
for this subject rather than stamped from a template.

## Step 0: adopt what already exists

Before designing anything, find the system the project already has. Cheapest
first:

```sh
rg -l "tailwind.config|theme.ts|tokens|:root\s*\{|@theme|design-system" --glob '!node_modules'
sed -n '1,80p' AGENTS.md CLAUDE.md 2>/dev/null
```

A tokens file, a theme, a component library, or existing sibling components
outrank every default below. Read two or three neighbouring components and
match their conventions: class strategy, spacing scale, naming, how variants
are expressed. Precedence is always the user's own words, then the project's
system, then your judgment. Only reach past the project when it has no answer.

New standalone page with no project to honor: pick the stack the request
implies and keep it minimal. Most pages need no framework and no library. Load
one only when it carries real weight, pinned to an exact version, and never
paste a library's source into the page.

## Calibrate the treatment

The question is never whether to design, only how loud. A memo, a plan, a
status page, an internal tool: polished and quiet. Real typographic hierarchy,
considered spacing, a proper palette, no gigantic hero, flourishes rare. A
landing page, a launch post, a game, something the user will keep or share:
editorial, opinionated, one real aesthetic risk.

When unsure, aim quiet. A well-composed page is never wrong; an over-designed
one sometimes is.

## Plan before code

Write down, in a few lines, the token system you are about to build to:

- **Color**: 4 to 6 named hex values, including the neutrals.
- **Type**: two or more roles. A display face used with restraint, a body face
  that complements it, a utility face for data and captions if the page has
  any.
- **Layout**: the composition in one or two sentences.

Then read the plan back against the subject. Any part that reads like the
default you would produce for any similar page gets revised before you write
code. Build from the revised plan and derive every color and type decision
from it.

## Fundamentals

**Ground it in the subject.** Pin one concrete subject, its audience, and the
page's single job. Distinctive choices come from the subject's own world: its
materials, instruments, vernacular. Carry at least one detail only this
subject would have, its real units and scales, its document conventions, its
terms of art, as content rather than ornament. Real content throughout, never
lorem.

**Typography carries the page,** even when the page is not about typography.
Set a type scale and stay on it. Running text near 65 characters wide.
`text-wrap: balance` on headings, letter-spacing on uppercase labels, body
text given room. Every webfont declares a real fallback stack and loads with
`display: swap`; a face that fails to load must degrade, not disappear.

**Choose the neutrals.** A pure mid-grey reads as unconsidered. Bias the greys
slightly toward the accent hue and they read as chosen. Pure white and
near-black are fine grounds when the subject wants them; the point is that
the neutral was picked, not inherited.

**Theme through tokens, never past them.** Define the complete palette as
custom properties on bare `:root`. Redefine only those tokens under
`@media (prefers-color-scheme: dark)`, and again under an explicit
`[data-theme]` stamp if the page has a toggle, so an explicit choice beats the
OS in both directions. Style components through the tokens. A color whose only
definition lives inside a media query or a `[data-theme]` block never applies
in the unstamped default state, which is the classic unreadable-page bug. Give
`body` an explicit background from a token rather than letting it inherit the
host's. Do not naively invert for dark; re-check contrast and that the accent
still works on the second ground. A page that deliberately commits to one
visual world may stay single-theme, but then it paints every color explicitly
so it holds on either host ground. Make that a choice, not an omission.

**Let layout do the spacing.** Sibling groups get flex or grid with `gap`, not
per-element margins that collapse or double. Wide content (tables, code,
diagrams) scrolls inside its own `overflow-x: auto` container so the body never
scrolls sideways. `font-variant-numeric: tabular-nums` anywhere digits line up.

**Compose repeated things as one object.** Cards in a row, label/value pairs
down a list, badges on siblings: identical edges, baselines and inner padding,
with recurring elements in the same place on each. Let content set container
height, and pick a column count the items actually fill so nothing stretches
over dead space or sits alone in a row. Text that can outgrow its track wraps
or scrolls; clipped text is a bug.

**Not everything is a card.** Border, fill, radius and shadow each say
"separate object". Spend them by role to lift the one thing that needs
lifting, instead of stamping one radius and one shadow on every block, which
flattens the hierarchy. Lead with big-number tiles only when those figures are
the point.

**Draw charts to the scale.** One scale places marks, ticks and labels, and
every label names a value the chart actually reaches. Chart text takes theme
tokens so it reads in both themes. Marks, labels and edges stay clear of each
other and inside the drawing bounds; in SVG leave viewBox room for the
outermost labels and give every shape an explicit fill.

**Show the page at rest.** Everything meant to be read is visible once the page
loads, without scrolling to trigger it. That first frame is what a screenshot,
a shared link, and a skimming reader all get. A section may animate in, but
from a visible resting state, never parked at `opacity: 0` waiting on an
observer. Size a hero to what it holds; a `100vh` opener pushes the page out of
its own first frame. A tool or app opens in a realistic working state, real
data where it exists and clearly-labelled example rows otherwise, so the first
look shows what it does. An empty shell shows nothing.

**When it is a UI, not a document.** A dashboard is scanned and operated, not
read top to bottom, so the craft shifts from typography to information design.
Summary before detail. Encode state in form as well as number (a pill, a chip,
a severity stripe) so what needs attention reads at a glance. Semantic color
(good / warning / critical) is separate from the accent and does not count as
it. Give sparklines the same care as type: an area fill, a faint grid, an
emphasized endpoint. What is interactive looks interactive.

**Copy is design material.** Write from the user's side of the screen and name
things by what people recognize, not how the system is built (a person manages
*notifications*, not *webhook config*). Active voice. A control says exactly
what happens ("Publish", then a toast reading "Published"). Errors say what
went wrong and what to do next, with no apology and no vagueness. Specific
beats clever.

**Structure is information.** Numbering, eyebrows, dividers and labels should
encode something true. Numbered markers (01 / 02 / 03) are right only when the
content genuinely is a sequence whose order the reader needs.

**Name it like a product.** A page title, a component name, a tab label: a
short specific noun phrase, typically two to four words. No explainer appended
after a dash or colon, and no generic category label that could sit on any
page. When trimming, the specific half is the half that survives.

## The accessibility floor

Non-negotiable, and cheap when done while building rather than after:

- Body text at 4.5:1 contrast against its own ground, large text and UI
  borders at 3:1. Check both themes, not just the one you designed first.
- Every interactive element is reachable by keyboard and has a visible focus
  state that is not the one you removed with `outline: none`.
- Interactive targets at least 24px, 44px on touch-first surfaces.
- Semantic elements before ARIA. A `<button>` before a `<div onclick>`, one
  `<h1>`, headings that descend without skipping.
- Every control has an accessible name; every meaningful image has alt text
  and every decorative one has `alt=""`.
- `@media (prefers-reduced-motion: reduce)` disables transforms and
  autoplaying motion, and never hides content that only appears after an
  animation.
- Color is never the only carrier of meaning; pair it with text, shape or
  icon.

## Build cleanly

Watch selector specificity: it is easy to generate classes that cancel each
other out, a `.section` rule fighting a `.cta` rule over the same padding.
Structure the cascade so it does not silently undo your own spacing. Close
every non-void element, quote every attribute, and check for overlapping
elements and silent font fallbacks. Give images explicit dimensions or an
aspect ratio so the page does not shift as they load. For generative or
decorative graphics reach for Canvas or WebGL rather than hand-authoring long
SVG path data.

## Avoid the generated look

Current AI design clusters hard around a handful of looks. Where the user pins
a direction, follow it exactly, including when they ask for one of these.
Where nothing is specified, do not spend that freedom here:

- warm cream `#F4F1EA` with a serif display and a terracotta accent
- near-black with a single acid-green or vermilion pop
- purple-to-blue gradient hero on white
- broadsheet hairline rules over dense columns
- Inter or Space Grotesk as the safe default face
- emoji as section markers, everything centered, `rounded-lg` on everything
- an accent bar or rail on every rounded card
- glassmorphism blur panels floating on a blurred blob background

Two structural tells beyond the palette: uniform elevation on every block, and
a hero sized to the viewport rather than to its content.

## Before you call it done

Write, look once, stop. If the project can render the page, take a single
screenshot and spend it on the thing most likely to be wrong (usually a chart,
a table, or the dark theme), then make one pass of edits. Do not build a test
loop around your own file; repeated screenshots and DOM-probing scripts spend
the session re-checking what a careful write already settled.

Last pass over the diff:

- [ ] Both themes resolve: no token defined only inside a media or
      `[data-theme]` block, `body` background explicit.
- [ ] Nothing clipped, nothing overlapping, no horizontal body scroll at
      narrow widths.
- [ ] Focus visible, contrast checked, reduced-motion honored.
- [ ] Every color and face traces back to the plan's tokens.
- [ ] Real content, no lorem, no placeholder figures presented as real data.

Further polish is the user's to ask for. If they report something visibly
broken, fix that and stop there.
