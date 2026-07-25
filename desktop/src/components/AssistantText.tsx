import { useEffect, useState } from "react";
import { Streamdown } from "streamdown";
import remarkMath from "remark-math";
import rehypeKatex from "rehype-katex";

// Turn ids whose reply has already been typed out, so re-renders and thread
// switches show the full text instead of re-animating.
const typedTurns = new Set<string>();

const shikiTheme: Parameters<typeof Streamdown>[0]["shikiTheme"] = [
  "github-light",
  "github-dark",
];

/** An assistant reply that types itself out on first appearance, then renders
 *  as normal markdown. Skips the animation for errors, replies already seen,
 *  and users who prefer reduced motion. Streamdown handles the streaming: it
 *  parses the still-incomplete markdown gracefully and defers Shiki/Mermaid on
 *  unclosed fences. */
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
  const [shown, setShown] = useState(animate ? 0 : text.length);

  useEffect(() => {
    typedTurns.add(id);
    if (!animate) {
      setShown(text.length);
      return;
    }
    // Scale speed to length so short replies feel deliberate and long ones
    // don't drag: a whole reply reveals in ~2s, clamped to a readable rate.
    const cps = Math.min(700, Math.max(220, text.length / 2));
    let raf = 0;
    let last = performance.now();
    const step = (now: number) => {
      const dt = now - last;
      last = now;
      setShown((s) => {
        const next = Math.min(text.length, s + (dt * cps) / 1000);
        if (next < text.length) raf = requestAnimationFrame(step);
        return next;
      });
    };
    raf = requestAnimationFrame(step);
    return () => cancelAnimationFrame(raf);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [id]);

  const done = shown >= text.length;
  const visible = done ? text : text.slice(0, Math.floor(shown));

  return (
    <div
      className={`a-text md${done ? "" : " typing"}`}
      style={error ? { color: "var(--red)" } : undefined}
    >
      <Streamdown
        parseIncompleteMarkdown
        isAnimating={!done}
        shikiTheme={shikiTheme}
        remarkPlugins={[remarkMath]}
        rehypePlugins={[rehypeKatex]}
      >
        {visible}
      </Streamdown>
    </div>
  );
}
