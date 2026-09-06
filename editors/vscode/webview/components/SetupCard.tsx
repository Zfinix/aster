import { useEffect, useState } from "react";
import type { ConnectAuth, Provider } from "../../src/protocol";
import { onHostMessage, post } from "../lib/host";
import type { LoginState } from "../lib/login";
import { keyPage, providerAuth, providerLabel } from "../lib/providers";
import { LoadingButton } from "../interior/loading-button";
import { KeyField } from "./KeyField";
import { ProviderList } from "./ProviderList";

type Step = { kind: "list" } | { kind: "key" | "login" | "none"; provider: Provider };

type Status =
  | { state: "idle" }
  | { state: "pending" }
  | { state: "failed"; message: string }
  | { state: "done" };

/** Onboarding in the composer's box: one list of providers, then the one thing
 *  that provider needs, right here. The host answers with a fresh init once the
 *  credentials are in, which is what turns this card into the greeting. */
export function SetupCard({
  login,
  providers = [],
}: {
  login: LoginState | null;
  providers?: Provider[];
}) {
  const [step, setStep] = useState<Step>({ kind: "list" });
  const [status, setStatus] = useState<Status>({ state: "idle" });
  const [key, setKey] = useState("");
  // The login prop is shared panel state, so a run started here is told apart
  // from an older result by the object it replaces.
  const [loginBefore, setLoginBefore] = useState<LoginState | null | undefined>(undefined);

  useEffect(() => {
    post({ type: "listProviders" });
  }, []);

  useEffect(
    () =>
      onHostMessage((msg) => {
        if (msg.type === "connectDone") {
          setStatus(msg.ok ? { state: "done" } : { state: "failed", message: msg.message });
        }
      }),
    []
  );

  const loginStatus = (): Status => {
    if (loginBefore !== undefined && login === loginBefore) return { state: "pending" };
    if (!login || !login.done) return { state: "pending" };
    if (login.ok) return { state: "done" };
    return { state: "failed", message: login.message ?? "The sign-in did not finish." };
  };
  const current: Status = step.kind === "login" ? loginStatus() : status;

  const connect = (provider: Provider, auth: ConnectAuth) => {
    if (auth.kind === "login") setLoginBefore(login);
    else setStatus({ state: "pending" });
    post({ type: "connect", baseUrl: provider.base_url, model: provider.example_model, auth });
  };

  const pick = (provider: Provider) => {
    const auth = providerAuth(provider);
    setKey("");
    setStatus({ state: "idle" });
    setStep({ kind: auth.kind, provider });
    if (auth.kind === "login") connect(provider, { kind: "login", target: auth.target });
    if (auth.kind === "none") connect(provider, { kind: "none" });
  };

  const retry = () => {
    if (step.kind === "list") return;
    const auth = providerAuth(step.provider);
    if (auth.kind === "key") connect(step.provider, { kind: "key", value: key.trim() });
    else if (auth.kind === "login") connect(step.provider, { kind: "login", target: auth.target });
    else connect(step.provider, { kind: "none" });
  };

  const back = () => {
    setStep({ kind: "list" });
    setStatus({ state: "idle" });
    setKey("");
  };

  const label = step.kind === "list" ? "" : providerLabel(step.provider).label;
  const busy = current.state === "pending";
  const { title, body } = copy(step, current, label);

  return (
    <div className="composer">
      <div className="setup-wrap" aria-busy={busy}>
        <div className="setup-title">{title}</div>
        <div className="setup-body">{body}</div>

        {step.kind === "list" && (
          <>
            <ProviderList providers={providers} onPick={pick} />
            <div className="setup-help">
              <span>
                Or run <code className="setup-code">aster init</code> in a terminal.
              </span>
            </div>
          </>
        )}

        {step.kind === "key" && current.state !== "done" && (
          <>
            <KeyField
              value={key}
              placeholder={`${label} API key`}
              disabled={busy}
              invalid={current.state === "failed"}
              onChange={setKey}
              onCommit={() => connect(step.provider, { kind: "key", value: key.trim() })}
            />
            {keyPage(step.provider) && (
              <div className="setup-hint">
                <button
                  type="button"
                  className="link"
                  onClick={() => post({ type: "openExternal", url: keyPage(step.provider) as string })}
                >
                  Get a key from {label}
                </button>
              </div>
            )}
          </>
        )}

        {step.kind === "login" && login && login.lines.length > 0 && current.state === "pending" && (
          <pre className="setup-card-log" aria-live="polite">
            {login.lines.join("\n")}
          </pre>
        )}

        {current.state === "failed" && (
          <div className="setup-card-failed" aria-live="polite">
            {current.message}
          </div>
        )}
      </div>

      {step.kind !== "list" && current.state !== "done" && (
        <div className="composer-foot">
          {step.kind === "key" && (
            <LoadingButton
              status={current.state === "failed" ? "error" : current.state === "pending" ? "pending" : "idle"}
              disabled={!key.trim()}
              idleLabel="Connect"
              pendingLabel="Checking…"
              errorLabel="Try again"
              onClick={() => connect(step.provider, { kind: "key", value: key.trim() })}
            />
          )}
          {step.kind !== "key" && current.state === "failed" && (
            <button type="button" className="btn-primary" onClick={retry}>
              Try again
            </button>
          )}
          <button type="button" className="ghost mode-btn" onClick={back}>
            Back
          </button>
        </div>
      )}
    </div>
  );
}

function copy(step: Step, status: Status, label: string): { title: string; body: string } {
  if (step.kind === "list") {
    return {
      title: "Connect a model",
      body: "Pick where Aster sends your requests. You can change this any time.",
    };
  }
  if (status.state === "done") {
    return {
      title: step.kind === "login" ? "Signed in" : "Connected",
      body: "Aster is ready.",
    };
  }
  if (step.kind === "login") {
    return status.state === "pending"
      ? {
          title: `Signing in with ${label}…`,
          body: "A browser window opened. Finish signing in there, then come back.",
        }
      : { title: `Sign in with ${label}`, body: "The browser sign-in did not finish." };
  }
  if (step.kind === "none") {
    return {
      title: status.state === "pending" ? `Connecting to ${label}…` : `Connect ${label}`,
      body: `Checking for a server at ${step.provider.base_url}.`,
    };
  }
  return {
    title: `Connect ${label}`,
    body:
      status.state === "pending"
        ? `Checking the key with ${label}…`
        : `Paste your ${label} API key. It stays on this machine.`,
  };
}
