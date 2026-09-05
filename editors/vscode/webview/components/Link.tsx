import type { ReactNode } from "react";

import { openFilePreview } from "../lib/filePreview";
import { post } from "../lib/host";

// `README.md:12` is a file and a line, not a scheme, hence the digit guard.
const SCHEME = /^[a-z][a-z0-9+.-]*:(?!\d)/i;
const LINE = /(?:#L|:)(\d+)(?:[-:,]L?\d+)*$/;

/** A link to a file rather than a page: `file://`, or a bare path with no
 *  scheme, either with an optional trailing `:42` or `#L42`. */
export function fileTarget(url: string): { path: string; line?: number } | undefined {
  let target: string;
  if (url.startsWith("file://")) {
    target = decodeURIComponent(url.slice("file://".length).replace(/^\/\/[^/]*/, ""));
  } else if (!SCHEME.test(url) && !url.startsWith("#")) {
    target = decodeURIComponent(url);
  } else {
    return undefined;
  }
  const line = LINE.exec(target);
  const path = line ? target.slice(0, line.index) : target;
  return path ? { path, line: line ? Number(line[1]) : undefined } : undefined;
}

/** A link in agent output. The click goes to the host rather than the
    webview's own navigation: a file opens in the editor, a `localhost` page
    is port-forwarded when the workspace is remote. */
export function Link({ url, children }: { url: string; children: ReactNode }) {
  const file = fileTarget(url);
  return (
    <a
      className="md-link"
      href={url}
      title={url}
      onClick={(e) => {
        e.preventDefault();
        if (file) {
          openFilePreview(file.path, file.line);
        } else {
          post({ type: "openExternal", url });
        }
      }}
    >
      {children}
    </a>
  );
}
