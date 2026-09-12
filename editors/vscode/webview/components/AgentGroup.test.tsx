import { renderToStaticMarkup } from "react-dom/server";
import { describe, expect, it } from "vitest";
import { AgentGroup } from "./AgentGroup";
import type { AgentTaskState } from "../lib/thread";

const task = (agent: string, over: Partial<AgentTaskState> = {}): AgentTaskState => ({
  callId: "c1",
  agent,
  task: "look at the bridge",
  status: "running",
  done: 0,
  total: 1,
  ...over,
});

describe("AgentGroup", () => {
  it("gives a lone sub-agent the same card a swarm gets", () => {
    const out = renderToStaticMarkup(<AgentGroup tasks={[task("scout")]} />);
    expect(out).toContain("agent-net-head");
    expect(out).toContain("Agents");
    expect(out).toContain("1 running");
    expect(out).toContain("agent-net-root");
  });

  it("counts a mixed batch", () => {
    const out = renderToStaticMarkup(
      <AgentGroup
        tasks={[
          task("scout", { status: "done", report: "it is in bridge.rs" }),
          task("scout"),
          task("prism", { status: "error", error: "Stopped: it repeats itself." }),
        ]}
      />
    );
    expect(out).toContain("1 running · 1 done · 1 failed");
  });
});
