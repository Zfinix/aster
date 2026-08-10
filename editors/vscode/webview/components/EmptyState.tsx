import { Mark } from "./Mark";

const INSTALL_CMD = "cargo install --path crates/aster-cli";

export function EmptyState({
  repoName,
  branch,
  binaryOk,
}: {
  repoName: string | null;
  branch: string | null;
  binaryOk: boolean;
}) {
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
      <h1 className="empty-title">What should we review next?</h1>
      {repoName ? (
        <p className="empty-body">
          {repoName}
          {branch && <span className="empty-branch"> · {branch}</span>}
        </p>
      ) : (
        <p className="empty-body">Open a folder to get started.</p>
      )}
    </div>
  );
}
