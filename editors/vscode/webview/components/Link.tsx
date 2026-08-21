import type { ReactNode } from "react";

import { post } from "../lib/host";

/** A link in agent output. The click goes to the host rather than the
    webview's own navigation, so a `file://` page opens at all and a
    `localhost` one is port-forwarded when the workspace is remote. */
export function Link({ url, children }: { url: string; children: ReactNode }) {
  return (
    <a
      className="md-link"
      href={url}
      title={url}
      onClick={(e) => {
        e.preventDefault();
        post({ type: "openExternal", url });
      }}
    >
      {children}
    </a>
  );
}
