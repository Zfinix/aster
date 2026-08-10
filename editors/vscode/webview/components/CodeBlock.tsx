import { post } from "../lib/host";
import { Code } from "./Code";
import { CopyButton } from "./CopyButton";
import { ExternalIcon } from "./icons";

/** Tapping opens the snippet as a real editor tab, for highlighting and find. */
export function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  // Never steal a selection the reader is making inside the block.
  const openInEditor = () => {
    if (window.getSelection()?.isCollapsed === false) return;
    post({ type: "openUntitled", content: code, lang, title: lang || "snippet" });
  };

  return (
    <div className="code-wrap">
      <div className="code-head">
        <span className="code-lang">{lang || "text"}</span>
        <button
          className="icon-btn"
          onClick={openInEditor}
          title="Open in editor"
          aria-label="Open in editor"
        >
          <ExternalIcon />
        </button>
        <CopyButton text={code} label="Copy code" />
      </div>
      <pre
        className="code-block"
        data-lang={lang || undefined}
        onClick={openInEditor}
        title="Click to open in an editor tab"
      >
        <code>
          <Code code={code} lang={lang} />
        </code>
      </pre>
    </div>
  );
}
