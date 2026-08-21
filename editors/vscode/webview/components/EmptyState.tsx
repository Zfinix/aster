import { Mark } from "./Mark";
import { Tip } from "./Tip";
import { OPENERS, TIPS, useRotation } from "../lib/greeting";

const INSTALL_CMD = "curl -fsSL https://withaster.dev/install | sh";

/** Long enough that a tip is never gone before it has been read, short enough
 *  that a panel left open shows more than one. */
const TIP_MS = 11000;

export function EmptyState({
  repoName,
  branch,
  binaryOk,
}: {
  repoName: string | null;
  branch: string | null;
  binaryOk: boolean;
}) {
  const opener = useRotation(OPENERS);
  const tip = useRotation(TIPS, TIP_MS);

  if (!binaryOk) {
    return (
      <div className="empty">
        <Mark px={2.6} />
        <h1 className="empty-title">Aster CLI not found</h1>
        <p className="empty-body">
          Reviews run through the <code>aster</code> binary. Install it, then reload the window.
        </p>
        <pre className="code-block">
          <code>{INSTALL_CMD}</code>
        </pre>
        <p className="empty-hint">
          Installed elsewhere? Set <code>aster.binaryPath</code>.
        </p>
      </div>
    );
  }

  return (
    <div className="empty">
      <Mark px={2.6} interactive />
      <h1 className="empty-title">{opener}</h1>
      {repoName ? (
        <p className="empty-body">
          {repoName}
          {branch && <span className="empty-branch"> · {branch}</span>}
        </p>
      ) : (
        <p className="empty-body">Open a folder to get started.</p>
      )}
      {/* Remounting on the text replays the fade, so a new tip arrives rather
          than swapping in place. */}
      <p className="empty-tip" key={tip}>
        <Tip text={tip} />
      </p>
    </div>
  );
}
