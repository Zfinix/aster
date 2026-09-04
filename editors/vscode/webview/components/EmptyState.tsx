import type { SetupInfo } from "../../src/protocol";
import type { LoginState } from "../lib/login";
import { Mark } from "./Mark";
import { SetupCard } from "./SetupCard";
import { Tip } from "./Tip";
import { OPENERS, TIPS, useRotation } from "../lib/greeting";

const INSTALL_CMD = "curl -fsSL https://withaster.dev/install | sh";

const TIP_MS = 11000;

export function EmptyState({
  repoName,
  branch,
  binaryOk,
  setup,
  login,
}: {
  repoName: string | null;
  branch: string | null;
  binaryOk: boolean;
  setup: SetupInfo | null;
  login: LoginState | null;
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

  if (setup) {
    return (
      <div className="empty">
        <Mark px={2.6} />
        <SetupCard setup={setup} login={login} />
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
