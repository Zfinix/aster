import { describe, expect, it } from "vitest";
import { renderToStaticMarkup } from "react-dom/server";

import { WebResults } from "./WebResults";

describe("WebResults", () => {
  it("renders each hit as a link over a dimmed URL and snippet", () => {
    const html = renderToStaticMarkup(
      <WebResults
        results={[
          { title: "Example", url: "https://example.com", snippet: "A page about things" },
          { title: "Second", url: "https://two.dev", snippet: "" },
        ]}
      />
    );
    expect(html).toContain('href="https://example.com"');
    expect(html).toContain("Example");
    expect(html).toContain("web-result-url");
    expect(html).toContain("A page about things");
    // The hit with an empty snippet omits the span rather than rendering an
    // empty one.
    expect(html.match(/web-result-snippet/g)).toHaveLength(1);
  });
});