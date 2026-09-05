import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";

const shikiTheme: Parameters<typeof Streamdown>[0]["shikiTheme"] = ["github-light", "github-dark"];
const plugins = { code };

/** An assistant reply as markdown. Streaming appends text and the block
 *  re-renders in place. */
export function AssistantText({ text, error }: { text: string; error?: boolean }) {
  return (
    <div className="prose" data-error={error || undefined}>
      <Streamdown
        plugins={plugins}
        shikiTheme={shikiTheme}
        animated={false}
        parseIncompleteMarkdown={false}
        remarkPlugins={[remarkMath]}
        rehypePlugins={[rehypeKatex]}
      >
        {text}
      </Streamdown>
    </div>
  );
}
