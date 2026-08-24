import { useLayoutEffect, useRef, useState } from "react";
import { post } from "../lib/host";
import type { AgentTaskState } from "../lib/thread";
import { Disclosure } from "../interior/disclosure";
import { AgentIcon, CheckIcon, ChevronIcon, ExternalIcon, SpinnerIcon, XIcon } from "./icons";
import { Markdown } from "./Markdown";

/** One wire of the graph, in pixel coordinates relative to `.agent-net-graph`. */
interface Wire {
  d: string;
  x1: number;
  y1: number;
  x2: number;
  y2: number;
  status: AgentTaskState["status"];
}

/** One `agent` tool call's swarm as a wired graph in a card: the orchestrator
 *  node fans out to a live node per sub-agent; a settled node opens its report. */
export function AgentGroup({ tasks }: { tasks: AgentTaskState[] }) {
  const [open, setOpen] = useState(true);
  const [selected, setSelected] = useState<string | null>(null);
  const graphRef = useRef<HTMLDivElement | null>(null);
  const rootRef = useRef<HTMLDivElement | null>(null);
  const nodeEls = useRef(new Map<string, HTMLElement>());
  const [wires, setWires] = useState<Wire[]>([]);

  const running = tasks.filter((t) => t.status === "running").length;
  const failed = tasks.filter((t) => t.status === "error").length;
  const settled = tasks.length - running;

  // The wires are measured off the rendered nodes rather than computed from
  // layout constants, so wrapped task text and theme fonts can't skew them.
  useLayoutEffect(() => {
    const graph = graphRef.current;
    if (!graph || !open) return;
    const measure = () => {
      const root = rootRef.current;
      if (!root) return;
      const box = graph.getBoundingClientRect();
      const from = root.getBoundingClientRect();
      const x1 = from.right - box.left;
      const y1 = from.top + from.height / 2 - box.top;
      setWires(
        tasks.flatMap((t) => {
          const el = nodeEls.current.get(`${tasks.indexOf(t)}:${t.agent}`);
          if (!el) return [];
          const to = el.getBoundingClientRect();
          const x2 = to.left - box.left;
          const y2 = to.top + to.height / 2 - box.top;
          const bend = Math.max((x2 - x1) / 2, 8);
          return [
            {
              d: `M ${x1} ${y1} C ${x1 + bend} ${y1}, ${x2 - bend} ${y2}, ${x2} ${y2}`,
              x1,
              y1,
              x2,
              y2,
              status: t.status,
            },
          ];
        })
      );
    };
    measure();
    const ro = new ResizeObserver(measure);
    ro.observe(graph);
    return () => ro.disconnect();
  }, [tasks, open]);

  const sel = tasks.find((t, i) => `${i}:${t.agent}` === selected);
  const report = sel?.report?.trim() ? sel.report : null;

  return (
    <div className="agent-net">
      <button className="agent-net-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="agent-net-title">
          <AgentIcon />
          Agents
        </span>
        <span className="agent-net-summary">
          {settled}/{tasks.length} done
          {failed > 0 && <span className="agent-net-failed"> · {failed} failed</span>}
        </span>
        <span className="agent-net-caret">
          <ChevronIcon open={open} />
        </span>
      </button>
      <Disclosure open={open}>
        <div className="agent-net-graph" ref={graphRef}>
          <svg className="agent-net-wires" aria-hidden="true">
            {wires.map((w, i) => (
              <g key={i} data-status={w.status}>
                <path className="agent-wire" d={w.d} />
                {w.status === "running" && <path className="agent-wire-flow" d={w.d} />}
                <circle className="agent-wire-end" cx={w.x2} cy={w.y2} r="2" />
              </g>
            ))}
            {wires[0] && (
              <circle className="agent-wire-port" cx={wires[0].x1} cy={wires[0].y1} r="2.5" />
            )}
          </svg>
          <div className="agent-net-root" ref={rootRef} data-running={running > 0}>
            <AgentIcon />
            <span>Agent</span>
          </div>
          <div className="agent-net-nodes">
            {tasks.map((t, i) => {
              const nodeId = `${i}:${t.agent}`;
              return (
                <AgentNode
                  key={nodeId}
                  task={t}
                  selected={selected === nodeId}
                  onSelect={() => setSelected(selected === nodeId ? null : nodeId)}
                  nodeRef={(el) => {
                    if (el) nodeEls.current.set(nodeId, el);
                    else nodeEls.current.delete(nodeId);
                  }}
                />
              );
            })}
          </div>
        </div>
        <Disclosure open={Boolean(report)}>
          <div className="agent-net-report">
            <div className="agent-net-report-head">
              <span className="agent-net-report-title">{sel?.agent} report</span>
              <button
                className="icon-btn"
                onClick={() =>
                  report &&
                  post({
                    type: "openUntitled",
                    content: report,
                    lang: "markdown",
                    title: sel?.agent ? `${sel.agent} report` : "report",
                  })
                }
                title="Open report in a markdown tab"
                aria-label="Open report in a markdown tab"
              >
                <ExternalIcon />
              </button>
            </div>
            {report && <Markdown text={report} />}
          </div>
        </Disclosure>
      </Disclosure>
    </div>
  );
}

function AgentNode({
  task,
  selected,
  onSelect,
  nodeRef,
}: {
  task: AgentTaskState;
  selected: boolean;
  onSelect: () => void;
  nodeRef: (el: HTMLButtonElement | null) => void;
}) {
  const hasReport = Boolean(task.report?.trim());
  const sub = task.status === "error" ? (task.error ?? "failed") : task.task;

  return (
    <button
      ref={nodeRef}
      className="agent-node"
      data-status={task.status}
      data-selected={selected}
      onClick={onSelect}
      disabled={!hasReport}
      aria-expanded={hasReport ? selected : undefined}
    >
      <span className="agent-node-dot" />
      <span className="agent-node-text">
        <span className="agent-node-name">{task.agent}</span>
        {sub && <span className="agent-node-task">{sub}</span>}
      </span>
      <span className="agent-node-state">
        {task.status === "running" ? (
          <SpinnerIcon />
        ) : task.status === "error" ? (
          <XIcon />
        ) : (
          <CheckIcon />
        )}
      </span>
    </button>
  );
}
