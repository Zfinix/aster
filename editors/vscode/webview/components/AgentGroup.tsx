import { useLayoutEffect, useRef, useState } from "react";
import type { ComponentType } from "react";
import { post } from "../lib/host";
import type { AgentTaskState } from "../lib/thread";
import { Disclosure } from "../interior/disclosure";
import {
  AgentIcon,
  CheckIcon,
  ChevronIcon,
  CompassIcon,
  ExternalIcon,
  HammerIcon,
  PencilIcon,
  RouteIcon,
  ShieldIcon,
  SparkleIcon,
  SpinnerIcon,
  XIcon,
} from "./icons";
import { Markdown } from "./Markdown";

/** Persona faces for the built-in agents; unknown names get the generic mark. */
const AVATARS: Record<string, ComponentType> = {
  scout: CompassIcon,
  cartographer: RouteIcon,
  sentinel: ShieldIcon,
  forge: HammerIcon,
  scribe: PencilIcon,
  prism: SparkleIcon,
};

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
  return tasks.length === 1 ? <AgentSolo task={tasks[0]} /> : <AgentSwarm tasks={tasks} />;
}

/** A batch of one has no swarm to wire, so the card is the node, a live tail
 *  of what it is doing, and its report. */
function AgentSolo({ task }: { task: AgentTaskState }) {
  const [open, setOpen] = useState(false);
  const report = task.report?.trim() ? task.report : null;
  const tail = task.status === "running" ? (task.log ?? []).slice(-6) : [];

  return (
    <div className="agent-net agent-net-solo">
      <AgentNode
        task={task}
        selected={open && Boolean(report)}
        onSelect={() => setOpen(!open)}
        showLive={false}
      />
      {tail.length > 0 && (
        <div className="agent-net-log">
          {tail.map((line, i) => (
            <div key={i} className="agent-net-log-line">
              {line}
            </div>
          ))}
        </div>
      )}
      <Disclosure open={open && Boolean(report)}>
        <AgentReport agent={task.agent} report={report} />
      </Disclosure>
    </div>
  );
}

function AgentSwarm({ tasks }: { tasks: AgentTaskState[] }) {
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
  const tail = !report && sel?.status === "running" ? (sel.log ?? []).slice(-8) : [];

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
        <Disclosure open={Boolean(report) || tail.length > 0}>
          {report ? (
            <AgentReport agent={sel?.agent} report={report} />
          ) : (
            <div className="agent-net-log agent-net-log-panel">
              {tail.map((line, i) => (
                <div key={i} className="agent-net-log-line">
                  {line}
                </div>
              ))}
            </div>
          )}
        </Disclosure>
      </Disclosure>
    </div>
  );
}

function AgentReport({ agent, report }: { agent?: string; report: string | null }) {
  return (
    <div className="agent-net-report">
      <div className="agent-net-report-head">
        <span className="agent-net-report-title">
          <span className="agent-net-report-name">{agent}</span> report
        </span>
        <button
          className="icon-btn"
          onClick={() =>
            report &&
            post({
              type: "openUntitled",
              content: report,
              lang: "markdown",
              title: agent ? `${agent} report` : "report",
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
  );
}

function AgentNode({
  task,
  selected,
  onSelect,
  nodeRef,
  showLive = true,
}: {
  task: AgentTaskState;
  selected: boolean;
  onSelect: () => void;
  nodeRef?: (el: HTMLButtonElement | null) => void;
  /** Off where a live tail is already visible, so the line isn't shown twice. */
  showLive?: boolean;
}) {
  const hasReport = Boolean(task.report?.trim());
  const live = task.status === "running" ? task.log?.at(-1) : undefined;
  const sub =
    task.status === "error" ? (task.error ?? "failed") : ((showLive ? live : undefined) ?? task.task);
  const hasBody = hasReport || Boolean(live);
  const Face = AVATARS[task.agent] ?? AgentIcon;

  return (
    <button
      ref={nodeRef}
      className="agent-node"
      data-status={task.status}
      data-selected={selected}
      onClick={onSelect}
      disabled={!hasBody}
      aria-expanded={hasBody ? selected : undefined}
    >
      <span className="agent-avatar">
        <Face />
        <span className="agent-avatar-dot" />
      </span>
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
