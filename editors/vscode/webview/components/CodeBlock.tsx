import { inEditor, post } from "../lib/host";
import { inPlanTab } from "../lib/plan-tab";
import { Code } from "./Code";
import { CopyButton } from "./CopyButton";
import { ExternalIcon } from "./icons";

/** Tapping opens the snippet as a real editor tab, for highlighting and find.
    A page has no tab to give it, so there it opens over the thread. */
export function CodeBlock({ code, lang }: { code: string; lang?: string }) {
  const label = inEditor ? "Open in editor" : "Open over the thread";
  // The plan's tab is a document on its own: there is no thread behind it to
  // open anything over, so the block is read and copied in place.
  const canOpen = !inPlanTab();
  // Never steal a selection the reader is making inside the block.
  const openInEditor = () => {
    if (window.getSelection()?.isCollapsed === false) return;
    post({ type: "openUntitled", content: code, lang, title: lang || "snippet" });
  };

  return (
    <div className="code-wrap">
      <div className="code-head">
        <span className="code-lang">{lang || "text"}</span>
        {canOpen && (
          <button
            className="icon-btn"
            onClick={openInEditor}
            title={label}
            aria-label={label}
          >
            <ExternalIcon />
          </button>
        )}
        <CopyButton text={code} label="Copy code" />
      </div>
      <pre
        className="code-block"
        data-lang={lang || undefined}
        onClick={canOpen ? openInEditor : undefined}
        title={
          canOpen
            ? inEditor
              ? "Click to open in an editor tab"
              : "Click to open over the thread"
            : undefined
        }
      >
        <code>
          <Code code={code} lang={lang} />
        </code>
      </pre>
    </div>
  );
}
