/** The wash behind tool blocks, by name. Hues are the accent of the same theme
 *  in the Aster TUI (crates/aster-cli/src/tui/palettes.rs), so the panel and the
 *  terminal read as one product. `steel` is the tokens.css default and `none`
 *  strips the wash so the border alone carries the block. Washes stay faint:
 *  text must read over them on a dark editor. */
const ACCENT_TONES: Record<string, [bg: string, strong: string]> = {
  steel: ["rgba(112, 144, 190, 0.07)", "rgba(112, 144, 190, 0.11)"],
  midnight: ["rgba(125, 196, 255, 0.07)", "rgba(125, 196, 255, 0.11)"],
  nord: ["rgba(136, 192, 208, 0.07)", "rgba(136, 192, 208, 0.11)"],
  forest: ["rgba(167, 192, 128, 0.07)", "rgba(167, 192, 128, 0.11)"],
  gruvbox: ["rgba(254, 128, 25, 0.07)", "rgba(254, 128, 25, 0.11)"],
  dracula: ["rgba(255, 121, 198, 0.07)", "rgba(255, 121, 198, 0.11)"],
  none: ["transparent", "transparent"],
};

export const ACCENTS = Object.keys(ACCENT_TONES);

/** Overrides the wash inline on the target (the document root in the app) so a
 *  change applies at once, without a rebuild and ahead of any stylesheet rule.
 *  The default clears the override instead, so each theme keeps the wash it
 *  tuned, and an unknown name falls back to that default rather than losing
 *  the wash. */
export function applyAccent(name: string | undefined, target: HTMLElement = document.documentElement): void {
  const tone = name && name in ACCENT_TONES ? name : "steel";
  if (tone === "steel") {
    target.style.removeProperty("--tool-bg");
    target.style.removeProperty("--tool-bg-strong");
    return;
  }
  const [bg, strong] = ACCENT_TONES[tone];
  target.style.setProperty("--tool-bg", bg);
  target.style.setProperty("--tool-bg-strong", strong);
}
