import { useEffect, useRef, useState } from "react";
import { Streamdown } from "streamdown";
import { code } from "@streamdown/code";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";

// Turn ids whose reply has already been typed out, so re-renders and thread
// switches show the full text instead of re-animating.
const typedTurns = new Set<string>();

const shikiTheme: Parameters<typeof Streamdown>[0]["shikiTheme"] = [
  "github-light",
  "github-dark",
];
const plugins = { code };

/** An assistant reply. On first appearance it fades itself in word by word via
 *  Streamdown's own animation, then renders as static markdown. The whole text
 *  is handed to Streamdown at once (Shiki highlights complete code immediately);
 *  we no longer re-parse a growing substring every frame, which was the source
 *  of the streaming stutter. Skips animation for errors, replies already seen,
 *  and reduced-motion. */
export function AssistantText({
  id,
  text,
  error,
}: {
  id: string;
  text: string;
  error?: boolean;
}) {
  const reduceMotion =
    typeof window !== "undefined" &&
    window.matchMedia?.("(prefers-reduced-motion: reduce)").matches;
  const animate = !error && !reduceMotion && !typedTurns.has(id);

  const [animating, setAnimating] = useState(animate);
  const timer = useRef<number>(0);

  useEffect(() => {
    typedTurns.add(id);
    if (!animate) return;
    const ms = Math.min(2600, Math.max(600, text.length * 6));
    timer.current = window.setTimeout(() => setAnimating(false), ms);
    return () => window.clearTimeout(timer.current);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  return (
    <div
      className={`a-text md${animating ? " typing" : ""}`}
      style={error ? { color: "var(--red)" } : undefined}
    >
      <Streamdown
        plugins={plugins}
        shikiTheme={shikiTheme}
        animated={animate ? { animation: "fadeIn", sep: "word", stagger: 14 } : false}
        isAnimating={animating}
        parseIncompleteMarkdown={false}
        remarkPlugins={[remarkMath]}
        rehypePlugins={[rehypeKatex]}
      >
        {text}
      </Streamdown>
    </div>
  );
}
