import { useCallback, useEffect, useState } from "react";
import { onHostMessage, post } from "../lib/host";
import { LoadingButton, type LoadingStatus } from "../interior/loading-button";
import { CopyButton } from "./CopyButton";

/** The same line the docs and the host's terminal install use. */
const INSTALL_CMD = "curl -fsSL https://withaster.dev/install | sh";

/** The CLI is missing, so onboarding wears the composer box the same way the
 *  key setup does: one card, one primary action, the host's output underneath. */
export function InstallCard() {
  const [status, setStatus] = useState<LoadingStatus>("idle");
  const [lines, setLines] = useState<string[]>([]);
  const [failure, setFailure] = useState<string | null>(null);

  useEffect(
    () =>
      onHostMessage((msg) => {
        if (msg.type === "installCliProgress") {
          setLines((prev) => [...prev, msg.message]);
        }
        if (msg.type === "installCliDone") {
          setStatus(msg.ok ? "success" : "error");
          setFailure(msg.ok ? null : msg.message);
        }
      }),
    []
  );

  const start = useCallback((message: { type: "installCli" } | { type: "installCliTerminal" }) => {
    setStatus("pending");
    setFailure(null);
    setLines([]);
    post(message);
  }, []);

  const pending = status === "pending";
  const title = pending
    ? "Downloading Aster CLI…"
    : status === "success"
      ? "Aster CLI installed"
      : "Aster CLI not found";

  return (
    <div className="composer">
      <div className="setup-wrap" aria-busy={pending}>
        <div className="setup-title">{title}</div>
        <div className="setup-body">
          Aster needs its CLI to review code and answer questions. The extension can
          download it for you.
        </div>
        {lines.length > 0 && (
          <pre className="setup-card-log" aria-live="polite">
            {lines.join("\n")}
          </pre>
        )}
        {status === "success" && (
          <div className="setup-card-ok" aria-live="polite">
            Installed. Ready to go.
          </div>
        )}
        {status === "error" && (
          <>
            <div className="setup-card-failed" aria-live="polite">
              {failure ? `Download failed: ${failure}` : "Download failed."}
            </div>
            <div className="setup-hint">
              <span>
                A terminal uses your own shell settings, so it gets through a proxy the
                editor cannot. Or run this yourself:
              </span>
              <div className="setup-command">
                <code>{INSTALL_CMD}</code>
                <CopyButton text={INSTALL_CMD} label="Copy the install command" />
              </div>
            </div>
          </>
        )}
      </div>
      <div className="composer-foot">
        {status === "error" ? (
          <button
            type="button"
            className="btn-primary"
            onClick={() => start({ type: "installCliTerminal" })}
          >
            Install in a terminal
          </button>
        ) : (
          <LoadingButton
            status={status}
            disabled={status === "success"}
            idleLabel="Install Aster CLI"
            pendingLabel="Installing…"
            successLabel="Installed"
            errorLabel="Try again"
            onClick={() => start({ type: "installCli" })}
          />
        )}
        <button
          type="button"
          className="ghost mode-btn"
          disabled={pending}
          onClick={() => post({ type: "locateCli" })}
        >
          Locate it myself
        </button>
      </div>
    </div>
  );
}
