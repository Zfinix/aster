import { createLibrary } from "@openuidev/react-lang";
import { createTheme } from "@openuidev/react-ui";
import { openuiChatLibrary } from "@openuidev/react-ui/genui-lib";
import { Card, Itinerary, Map, Stop } from "./openui-places";

/** The OpenUI chat components Aster renders besides its own place components.
 *  A subset, so the spec the model is taught stays near 3K tokens, not 13K. */
const COMPONENTS = [
  "CardHeader",
  "TextContent",
  "Callout",
  "CodeBlock",
  "Image",
  "ImageGallery",
  "Table",
  "Col",
  "BarChart",
  "LineChart",
  "PieChart",
  "Series",
  "Slice",
  "Form",
  "FormControl",
  "Label",
  "Input",
  "TextArea",
  "Select",
  "SelectItem",
  "RadioGroup",
  "RadioItem",
  "CheckBoxGroup",
  "CheckBoxItem",
  "Button",
  "Buttons",
  "Steps",
  "StepsItem",
  "Tabs",
  "TabItem",
  "Accordion",
  "AccordionItem",
  "TagBlock",
  "Tag",
  "FollowUpBlock",
  "FollowUpItem",
  "OptionCards",
  "OptionCard",
] as const;

const chat = openuiChatLibrary.components as Record<string, (typeof openuiChatLibrary.components)[string]>;

export const library = createLibrary({
  root: "Card",
  components: [Card, Map, Itinerary, Stop, ...COMPONENTS.flatMap((name) => chat[name] ?? [])],
});

/** OpenUI's surfaces and text, pointed at the webview's tokens so a block
 *  follows the VS Code theme; spacing, radii, type and shadows stay OpenUI's. */
export const theme = createTheme({
  background: "var(--bg)",
  foreground: "var(--surface)",
  popoverBackground: "var(--surface)",
  sunkLight: "var(--code-bg-soft)",
  sunk: "var(--code-bg)",
  sunkDeep: "var(--code-bg-strong)",
  elevatedLight: "var(--code-bg-soft)",
  elevated: "var(--surface)",
  elevatedStrong: "var(--code-bg-strong)",
  highlightSubtle: "var(--code-bg-soft)",
  highlight: "var(--list-hover)",
  highlightStrong: "var(--code-bg-strong)",
  textNeutralPrimary: "var(--fg)",
  textNeutralSecondary: "var(--fg-dim)",
  textNeutralTertiary: "var(--fg-dim)",
  textNeutralLink: "var(--vscode-textLink-foreground, var(--fg))",
  textBrand: "var(--fg)",
  textAccentPrimary: "var(--vscode-button-foreground)",
  interactiveAccentDefault: "var(--vscode-button-background)",
  interactiveAccentHover: "var(--vscode-button-hoverBackground)",
  interactiveAccentPressed: "var(--vscode-button-background)",
  borderDefault: "var(--border)",
  borderInteractive: "var(--border)",
  borderInteractiveSelected: "var(--vscode-focusBorder)",
  borderAccent: "var(--vscode-focusBorder)",
  fontBody: "var(--vscode-font-family)",
  fontHeading: "var(--vscode-font-family)",
  fontLabel: "var(--vscode-font-family)",
  fontCode: "var(--mono)",
});

/** Longest form-answers note sent with a button press, so a pasted text field
 *  cannot flood the context. */
const MAX_ANSWERS_CHARS = 1000;

function answerText(value: unknown): string | null {
  if (value == null || value === "") return null;
  if (Array.isArray(value)) return value.map(String).join(", ") || null;
  if (typeof value === "object") {
    const picked = Object.entries(value).filter(([, on]) => on === true).map(([name]) => name);
    return picked.length ? picked.join(", ") : null;
  }
  return String(value);
}

function collect(state: Record<string, unknown>, out: string[]): void {
  for (const [name, entry] of Object.entries(state)) {
    if (name.startsWith("$") || !entry || typeof entry !== "object") continue;
    if ("value" in entry) {
      const text = answerText((entry as { value: unknown }).value);
      if (text) out.push(`${name}: ${text}`);
    } else {
      collect(entry as Record<string, unknown>, out);
    }
  }
}

/** The button's message plus what the user filled in, as one line the model reads. */
export function withAnswers(message: string, formState?: Record<string, unknown>): string {
  const answers: string[] = [];
  if (formState) collect(formState, answers);
  if (!answers.length) return message;
  const note = answers.join("; ");
  const clipped = note.length > MAX_ANSWERS_CHARS ? `${note.slice(0, MAX_ANSWERS_CHARS)}…` : note;
  return `${message}\n\n(Form answers: ${clipped})`;
}
