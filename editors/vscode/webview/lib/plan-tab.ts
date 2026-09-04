/** A plan handed to a tab of its own under `aster serve`. localStorage is the
 *  one channel both same-origin tabs can see, so the plan and its answer both
 *  travel that way; the thread's tab owns the session and replies to the CLI. */

const PLAN = "aster.plan";
const ANSWER = "aster.plan.answer";
const TARGET = "aster-plan";

/** The plan's own tab, which has a document and nothing else: no thread to
 *  open a snippet over, and no session of its own. */
export function inPlanTab(): boolean {
  return window.location.pathname === "/plan";
}

export interface PlanAnswer {
  allow: boolean;
  nonce: number;
}

/** True once the tab is open. False when a popup blocker ate it, which is the
 *  caller's cue to show the plan where it stands instead. */
export function openPlanTab(markdown: string): boolean {
  try {
    localStorage.setItem(PLAN, markdown);
  } catch {
    return false;
  }
  return window.open("/plan", TARGET) !== null;
}

export function readPlan(): string {
  try {
    return localStorage.getItem(PLAN) ?? "";
  } catch {
    return "";
  }
}

/** The thread answered, so the plan is no longer open for one. */
export function closePlan(): void {
  try {
    localStorage.removeItem(PLAN);
  } catch {
    // A blocked store leaves the other tab showing a stale plan, which its own
    // answer still resolves.
  }
}

export function sendPlanAnswer(allow: boolean): void {
  try {
    localStorage.setItem(ANSWER, JSON.stringify({ allow, nonce: Date.now() }));
  } catch {
    // Nothing to fall back to: the thread's own card still holds the decision.
  }
}

export function onPlanAnswer(handler: (answer: PlanAnswer) => void): () => void {
  return onStorage(ANSWER, (value) => {
    try {
      handler(JSON.parse(value) as PlanAnswer);
    } catch {
      // Not ours, or half-written.
    }
  });
}

/** A plan arriving, or being cleared once the thread answers. */
export function onPlanChange(handler: (markdown: string) => void): () => void {
  return onStorage(PLAN, handler, true);
}

function onStorage(key: string, handler: (value: string) => void, empty = false): () => void {
  const listener = (e: StorageEvent) => {
    if (e.key !== key) return;
    if (e.newValue === null && !empty) return;
    handler(e.newValue ?? "");
  };
  window.addEventListener("storage", listener);
  return () => window.removeEventListener("storage", listener);
}
