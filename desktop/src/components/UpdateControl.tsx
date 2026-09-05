import type { UpdateStage } from "../lib/updater";

export function UpdateControl({
  stage,
  onCheck,
  onInstall,
}: {
  stage: UpdateStage;
  onCheck: () => void;
  onInstall: () => void;
}) {
  switch (stage.kind) {
    case "checking":
      return <span className="settings-note">Checking…</span>;
    case "none":
      return <span className="settings-note">Up to date</span>;
    case "available":
      return (
        <button type="button" className="btn-primary" onClick={onInstall}>
          Update to {stage.version}
        </button>
      );
    case "downloading": {
      const pct = stage.total ? Math.round((stage.done / stage.total) * 100) : null;
      return <span className="settings-note">Downloading{pct !== null ? ` ${pct}%` : "…"}</span>;
    }
    case "ready":
      return <span className="settings-note">Restarting…</span>;
    case "error":
      return (
        <button type="button" className="btn" onClick={onCheck} title={stage.message}>
          Try again
        </button>
      );
    default:
      return (
        <button type="button" className="btn" onClick={onCheck}>
          Check for updates
        </button>
      );
  }
}
