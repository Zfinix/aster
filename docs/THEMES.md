# Themes

The chat TUI is fully themeable. `/theme` opens a picker; arrowing through it
previews each palette live and Enter saves the choice to `ui.theme` in
`aster.yaml`. `/theme <name>` switches directly. Esc in the picker restores
what you had.

## Built-in themes

| Name | Family | Look |
| --- | --- | --- |
| `dark` | aster default | warm dark, ember accent |
| `light` | aster default | for bright terminals |
| `midnight` | aster default | deep blue, Tokyo-night family |
| `forest` | aster default | muted green, everforest family |
| `dracula` | Dracula | purple background, pink and purple accents |
| `catppuccin` | Catppuccin Mocha | warm pastels on deep mauve |
| `nord` | Nord | cool blues on slate |
| `gruvbox` | Gruvbox Dark | earthy amber and orange |
| `solarized` | Solarized Dark | muted teal and blue |
| `synthwave` | Synthwave '84 | neon pink to cyan on dark purple |
| `github-dark` | GitHub Dark | calm blues, dimmed backgrounds |
| `monokai` | Monokai Pro | rich orange, green, aqua |
| `one-dark` | Atom One Dark | blues, mint, pink |
| `aster-ocean` | aster | deep teal water, cyan accent |
| `aster-ember` | aster | warm charcoal, burning orange |
| `aster-orchid` | aster | violet dark, magenta accent |

## Custom themes

Drop a YAML file into `~/.aster/themes/` (all projects) or `.aster/themes/`
in a repo (that repo only). A project file shadows a global one with the same
name. The picker picks them up on the next launch, capped at 64 files.

A theme file can be one line. Missing fields inherit from `base`:

```yaml
# ~/.aster/themes/sunset.yaml
base: aster-ember
accent: "#ff9a3c"
```

Every field of the palette is overridable by its YAML key. Colors are hex,
with or without the leading `#`:

```yaml
# ~/.aster/themes/nightshift.yaml
name: nightshift
description: low-blue dark for late sessions
base: midnight
text: "#e8e8e8"
dim: "#8a8fa0"
accent: "#7dc4ff"
error: "#ef5a6f"
success: "#7eab6a"
sel_bg: "#1a2a4a"
pane_bg: "#0f1422"
mark:
  - "#7dc4ff"
  - "#68dfff"
```

| Key | Palette field |
| --- | --- |
| `text` | body text |
| `dim` / `dimmer` / `faint` | de-emphasized text, strongest to weakest |
| `accent` | the signature color: mode chip, selection, highlights |
| `error` / `success` / `warning` | status colors |
| `rail_bg` / `pane_bg` / `sel_bg` | surfaces: rail, panes, selected rows |
| `amber` / `blue` / `purple` | secondary accents |
| `add_bg` / `add_fg` / `add_mark` | diff added rows |
| `del_bg` / `del_fg` / `del_mark` | diff removed rows |
| `inline_code_bg` / `inline_code_fg` | inline code spans |
| `heading_fg` / `link_fg` / `placeholder` | markdown rendering |
| `severity_critical` ... `severity_info` | review severity colors |
| `mark` | the 10-stop asterisk gradient, as a list |

`name:` overrides the file stem (spaces become dashes), `description:` is the
line shown in the picker, and `base:` names any built-in from the table above.
