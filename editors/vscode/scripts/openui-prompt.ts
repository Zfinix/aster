// Writes crates/aster-cli/prompts/openui.md: the OpenUI section of the system
// prompt, generated from the components the webview registers so the model is
// never taught one the panel cannot draw. Run with `bun run prompt:openui`.
import { writeFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { library } from "../webview/lib/openui";

const out = fileURLToPath(new URL("../../../crates/aster-cli/prompts/openui.md", import.meta.url));

const preamble = `## Rich replies

This client draws OpenUI Lang, so a reply can carry real UI: cards, tables, charts, steps, tabs and forms. Write replies in markdown as usual. When a visual answer is clearly better than prose, such as comparing options, numbers over time, a plan to follow, or a choice the user should make, add a fenced \`\`\`openui block. Text before and after the block stays markdown, and the block draws in place while it streams.`;

const rules = [
  "Most replies need no block. Add one only when it shows something prose cannot show as well.",
  "Code, diffs, commands, logs and file contents always go in markdown code fences, never inside an openui block.",
  "Only show numbers and facts that came from the user, a tool result, or the repository. Never invent metrics to fill a chart or table.",
  "Do not repeat in prose what the block already shows; a sentence before or after is enough.",
  "A Button with Action([@ToAssistant(\"...\")]) sends that text back as the user's next message, with any form answers attached, so word it as the user would.",
  "For places (a trip, venues, offices, anything with a location), show a Map and an Itinerary that share the same Stop references. Before writing them, call web/images once per stop and copy a returned URL into Stop.image; leave image out when nothing came back.",
  "Every image URL (Stop.image, Image, ImageGallery) must be one web/images returned, copied exactly. Never guess or build an image URL.",
];

const registered = new Set(Object.keys(library.components));

const prompt = library
  .prompt({ preamble, additionalRules: rules })
  // OpenUI lists every chat component in its unions; keep the ones registered here.
  .replace(/\b[A-Z]\w*(?: \| [A-Z]\w*)+\b/g, (union) => {
    const kept = union.split(" | ").filter((name) => registered.has(name));
    return kept.length ? kept.join(" | ") : union;
  })
  // Nest OpenUI's own sections under the preamble's heading.
  .replace(/^(#{2,5}) /gm, "#$1 ")
  .replace(/^### Rich replies$/m, "## Rich replies");

writeFileSync(out, `${prompt.trim()}\n`);
console.log(`wrote ${out}: ${prompt.length} chars, about ${Math.round(prompt.length / 4)} tokens`);
