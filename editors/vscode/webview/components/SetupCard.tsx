import type { SetupInfo } from "../../src/protocol";
import { inEditor, post } from "../lib/host";
import type { LoginState } from "../lib/login";
import { setupCopy } from "../lib/login";

/**
 * Shown in place of an error when the endpoint has no credentials yet: a
 * sign-in button when a browser login exists, else where the key goes. The
 * login's own output streams underneath so the user can follow the browser flow.
 */
export function SetupCard({ setup, login }: { setup: SetupInfo; login: LoginState | null }) {
  const copy = setupCopy(setup);
  const running = login !== null && !login.done;

  return (
    <div className="setup-card">
      <div className="setup-card-title">{copy.title}</div>
      <div className="setup-card-body">{copy.body}</div>
      <div className="setup-card-actions">
        {copy.signIn && setup.login && (
          <button
            type="button"
            className="btn-primary"
            disabled={running}
            onClick={() => post({ type: "login", target: setup.login as string })}
          >
            {running ? "Waiting for the browser…" : copy.signIn}
          </button>
        )}
        {copy.keyVar && inEditor && (
          <button
            type="button"
            className="btn"
            onClick={() => post({ type: "runCommand", command: "aster.openSettings" })}
          >
            Add an API key
          </button>
        )}
      </div>
      {copy.keyVar && !inEditor && (
        <div className="setup-card-hint">
          Or set the key in the terminal running <code>aster serve</code>:
          <pre className="code-block">
            <code>aster key set {copy.keyVar}</code>
          </pre>
        </div>
      )}
      {login && login.lines.length > 0 && (
        <pre className="setup-card-log">{login.lines.join("\n")}</pre>
      )}
      {login?.done && (
        <div className={login.ok ? "setup-card-ok" : "setup-card-failed"}>
          {login.ok ? "Signed in. Send your message again." : login.message}
        </div>
      )}
    </div>
  );
}
