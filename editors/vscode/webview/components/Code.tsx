import { Fragment } from "react";
import { useTokens, type ThemedToken } from "../lib/highlight";

/** One tokenised line. Each span carries both themes' colors as custom
 *  properties; the stylesheet picks a side from the body's theme class. */
export function TokenLine({ tokens }: { tokens: ThemedToken[] }) {
  return (
    <>
      {tokens.map((token, i) => (
        <span key={i} className="sh" style={token.htmlStyle}>
          {token.content}
        </span>
      ))}
    </>
  );
}

/** Highlighted source, falling back to plain text for unknown languages and for
 *  the first frames before the grammars finish loading. */
export function Code({ code, lang }: { code: string; lang?: string }) {
  const tokens = useTokens(code, lang);
  if (!tokens) return <>{code}</>;

  return (
    <>
      {tokens.map((line, i) => (
        <Fragment key={i}>
          {i > 0 && "\n"}
          <TokenLine tokens={line} />
        </Fragment>
      ))}
    </>
  );
}
