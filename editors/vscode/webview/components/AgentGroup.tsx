import { useLayoutEffect, useRef, useState } from "react";
import type { ComponentType } from "react";
import { post } from "../lib/host";
import type { AgentTaskState } from "../lib/thread";
import { describeActivity, elapsedLabel } from "../lib/tools";
import { useNow } from "../lib/useNow";
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

const AVATARS: Record<string, ComponentType> = {
  scout: CompassIcon,
  cartographer: RouteIcon,
  sentinel: ShieldIcon,
  forge: HammerIcon,
  scribe: PencilIcon,
  prism: SparkleIcon,
};

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

function actionCount(task: AgentTaskState): number {
  return (task.log ?? []).filter((line) => describeActivity(line).kind === "tool").length;
}

function elapsed(task: AgentTaskState, now: number): string | undefined {
  if (!task.startedAt) return undefined;
  const end = task.status === "running" ? now : (task.endedAt ?? now);
  return elapsedLabel(end - task.startedAt);
}

function AgentSolo({ task }: { task: AgentTaskState }) {
  const [open, setOpen] = useState(false);
  const now = useNow(task.status === "running");
  const report = task.report?.trim() ? task.report : null;
  const tail = task.status === "running" ? (task.log ?? []).slice(-6) : [];

  return (
    <div className="agent-net agent-net-solo">
      <AgentNode
        task={task}
        now={now}
        selected={open && Boolean(report)}
        onSelect={() => setOpen(!open)}
      />
      {tail.length > 0 && <ActivityLog lines={tail} />}
      <Disclosure open={open && Boolean(report)}>
        <AgentReport agent={task.agent} report={report} log={task.log ?? []} />
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
  const actions = tasks.reduce((sum, t) => sum + actionCount(t), 0);
  const now = useNow(running > 0);
  const started = Math.min(...tasks.map((t) => t.startedAt ?? Infinity));
  const ended = Math.max(...tasks.map((t) => t.endedAt ?? 0));
  const clock =
    Number.isFinite(started) && (running > 0 || ended > 0)
      ? elapsedLabel((running > 0 ? now : ended) - started)
      : undefined;

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
          {settled} of {tasks.length} done
          {failed > 0 && <span className="agent-net-failed"> · {failed} failed</span>}
          {actions > 0 && ` · ${actions} ${actions === 1 ? "action" : "actions"}`}
          {clock && ` · ${clock}`}
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
                  now={now}
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
            <AgentReport agent={sel?.agent} report={report} log={sel?.log ?? []} />
          ) : (
            <ActivityLog lines={tail} panel />
          )}
        </Disclosure>
      </Disclosure>
    </div>
  );
}

/** The live tail of what an agent did, newest last: each tool call as a verb
 *  and its target, the agent's own commentary in between. */
function ActivityLog({
  lines,
  panel,
  steps,
}: {
  lines: string[];
  panel?: boolean;
  steps?: boolean;
}) {
  const className = steps
    ? "agent-net-log agent-net-log-steps"
    : panel
      ? "agent-net-log agent-net-log-panel"
      : "agent-net-log";
  return (
    <div className={className}>
      {lines.map((line, i) => {
        const item = describeActivity(line);
        return item.kind === "tool" ? (
          <div key={i} className="agent-net-log-line">
            <span className="agent-net-log-verb">{item.verb}</span>
            {item.detail && <span className="agent-net-log-detail">{item.detail}</span>}
          </div>
        ) : (
          <div key={i} className="agent-net-log-line agent-net-log-note">
            {item.text}
          </div>
        );
      })}
    </div>
  );
}

/** The report, with what the agent did to get there listed above it, so the
 *  conclusion can be checked against the steps. */
function AgentReport({
  agent,
  report,
  log,
}: {
  agent?: string;
  report: string | null;
  log: string[];
}) {
  const steps = log.filter((line) => describeActivity(line).kind === "tool");
  return (
    <div className="agent-net-report">
      {steps.length > 0 && <ActivityLog lines={steps} steps />}
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

/** One sub-agent: who it is, what it was asked, and what it is doing right
 *  now. The ask stays put while the live line under it changes. */
function AgentNode({
  task,
  now,
  selected,
  onSelect,
  nodeRef,
}: {
  task: AgentTaskState;
  now: number;
  selected: boolean;
  onSelect: () => void;
  nodeRef?: (el: HTMLButtonElement | null) => void;
}) {
  const hasReport = Boolean(task.report?.trim());
  const live = task.status === "running" ? task.log?.at(-1) : undefined;
  const hasBody = hasReport || Boolean(live);
  const Face = AVATARS[task.agent] ?? AgentIcon;
  const actions = actionCount(task);
  const time = elapsed(task, now);
  const meta = [
    task.status === "error" ? "failed" : task.status === "done" ? "done" : undefined,
    actions > 0 ? `${actions} ${actions === 1 ? "action" : "actions"}` : undefined,
    time,
  ]
    .filter(Boolean)
    .join(" · ");
  const current = live ? describeActivity(live) : undefined;

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
        <span className="agent-node-head">
          <span className="agent-node-name">{task.agent}</span>
          {meta && <span className="agent-node-meta">{meta}</span>}
        </span>
        {task.task && (
          <span className="agent-node-task" title={task.task}>
            {task.task}
          </span>
        )}
        {task.status === "error" && task.error && (
          <span className="agent-node-error">{task.error}</span>
        )}
        {current && (
          <span className="agent-node-live">
            {current.kind === "tool" ? (
              <>
                <span className="agent-node-live-verb">{current.verb}</span>
                {current.detail && <span className="agent-node-live-detail">{current.detail}</span>}
              </>
            ) : (
              <span className="agent-node-live-note">{current.text}</span>
            )}
          </span>
        )}
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
