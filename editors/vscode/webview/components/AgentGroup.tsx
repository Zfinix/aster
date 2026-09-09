import { useLayoutEffect, useRef, useState } from "react";
import { AnimatePresence, motion, useReducedMotion } from "motion/react";
import { post } from "../lib/host";
import type { AgentTaskState } from "../lib/thread";
import { describeActivity, elapsedLabel, runLabel } from "../lib/tools";
import { useNow } from "../lib/useNow";
import { Disclosure } from "../interior/disclosure";
import { CELL, INSTANT } from "../interior/springs";
import {
  AgentIcon,
  AlertIcon,
  CheckIcon,
  ChevronIcon,
  ExternalIcon,
  SpinnerIcon,
  XIcon,
} from "./icons";
import { toolIcon, writesFiles } from "./toolIcons";
import { Markdown } from "./Markdown";

const WORDS: Record<AgentTaskState["status"], string> = {
  running: "running",
  done: "done",
  error: "failed",
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

/** What an agent spent its steps on, by tool, in the order it first reached for
 *  each: "four reads and an edit" is the shape of a run, "seven commands" another. */
function tally(log: string[]): { name: string; count: number }[] {
  const counts = new Map<string, number>();
  for (const line of log) {
    const item = describeActivity(line);
    if (item.kind !== "tool") continue;
    counts.set(item.name, (counts.get(item.name) ?? 0) + 1);
  }
  return [...counts].map(([name, count]) => ({ name, count }));
}

/** The opening claim of a report, so a finished row says what it concluded
 *  rather than repeating the ask. Headings and fences are chrome, not the answer. */
function gist(report: string): string | undefined {
  for (const raw of report.split("\n")) {
    const line = raw.trim();
    if (!line || line.startsWith("#") || line.startsWith("```") || line.startsWith("|")) continue;
    const plain = line.replace(/^[-*+]\s+/, "").replace(/[*_`>]/g, "").trim();
    if (plain) return plain;
  }
  return undefined;
}

/** A failure as what went wrong and what to do about it: the CLI writes these
 *  as one run-on ("Stopped: it repeats itself. Try a stronger model."), and the
 *  advice is the half the reader acts on. */
function failure(error: string): { what: string; fix?: string } {
  const trimmed = error.trim();
  const split = /^([\s\S]*?[.;!?])\s+([\s\S]+)$/.exec(trimmed);
  if (!split) return { what: sentence(trimmed) };
  return { what: sentence(split[1]), fix: sentence(split[2]) };
}

function sentence(text: string): string {
  const trimmed = text.trim().replace(/;$/, ".");
  const capped = trimmed.charAt(0).toUpperCase() + trimmed.slice(1);
  return /[.!?]$/.test(capped) ? capped : `${capped}.`;
}

function elapsed(task: AgentTaskState, now: number): string | undefined {
  if (!task.startedAt) return undefined;
  const end = task.status === "running" ? now : (task.endedAt ?? now);
  return elapsedLabel(end - task.startedAt);
}

function AgentSolo({ task }: { task: AgentTaskState }) {
  const [open, setOpen] = useState(false);
  const now = useNow(task.status === "running");

  return (
    <div className="agent-net agent-net-solo">
      <AgentNode task={task} selected={open} onSelect={() => setOpen(!open)} />
      <Disclosure open={open}>
        <AgentPanel task={task} now={now} />
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
  const done = tasks.filter((t) => t.status === "done").length;
  const actions = tasks.reduce((sum, t) => sum + actionCount(t), 0);
  const now = useNow(running > 0);
  const started = Math.min(...tasks.map((t) => t.startedAt ?? Infinity));
  const ended = Math.max(...tasks.map((t) => t.endedAt ?? 0));
  const clock =
    Number.isFinite(started) && (running > 0 || ended > 0)
      ? elapsedLabel((running > 0 ? now : ended) - started)
      : undefined;

  const counts = [
    running > 0 && `${running} running`,
    done > 0 && `${done} done`,
    failed > 0 && `${failed} failed`,
  ]
    .filter(Boolean)
    .join(" · ");
  const work = [actions > 0 && `${actions} ${actions === 1 ? "action" : "actions"}`, clock]
    .filter(Boolean)
    .join(" · ");

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

  return (
    <div className="agent-net">
      <button className="agent-net-head" onClick={() => setOpen(!open)} aria-expanded={open}>
        <span className="agent-net-title">
          <AgentIcon />
          Agents
        </span>
        <span className="agent-net-counts">{counts}</span>
        <span className="agent-net-summary">{work}</span>
        <span className="agent-net-caret">
          <ChevronIcon open={open} />
        </span>
      </button>
      <Progress done={done} failed={failed} total={tasks.length} />
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
        <Disclosure open={Boolean(sel)}>{sel && <AgentPanel task={sel} now={now} />}</Disclosure>
      </Disclosure>
    </div>
  );
}

/** Real progress, not a spinner: the bar is the share of the batch that has
 *  landed, and it advances by one agent's worth each time one finishes. */
function Progress({ done, failed, total }: { done: number; failed: number; total: number }) {
  const reduced = useReducedMotion();
  const settled = done + failed;

  return (
    <div
      className="agent-net-progress"
      role="progressbar"
      aria-valuemin={0}
      aria-valuemax={total}
      aria-valuenow={settled}
      aria-label={`${settled} of ${total} agents finished`}
    >
      <motion.span
        className="agent-net-progress-fill"
        initial={false}
        animate={{ scaleX: settled / total }}
        transition={reduced ? INSTANT : CELL}
      />
    </div>
  );
}

/** What an agent did, step by step: each tool call as a verb and its target,
 *  the agent's own commentary in between. */
function ActivityLog({ lines }: { lines: string[] }) {
  return (
    <div className="agent-net-log">
      {lines.map((line, i) => {
        const item = describeActivity(line);
        return item.kind === "tool" ? (
          <div key={i} className="agent-net-log-line">
            <span className="agent-net-log-icon">{toolIcon(item.name)}</span>
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

/** A chip per tool the agent reached for, counted: a run that only read is a
 *  different thing from one that wrote, and this says which in one look. */
function StepTally({ log }: { log: string[] }) {
  const steps = tally(log);
  if (steps.length === 0) return null;
  return (
    <span className="agent-steps">
      {steps.map(({ name, count }) => (
        <span
          key={name}
          className="agent-step"
          data-write={writesFiles(name)}
          title={runLabel(name, count)}
        >
          {toolIcon(name)}
          {count}
        </span>
      ))}
    </span>
  );
}

/** Everything about one sub-agent that the row leaves out: the ask it was
 *  given, what it did, and what it handed back. */
function AgentPanel({ task, now }: { task: AgentTaskState; now: number }) {
  const report = task.report?.trim() ? task.report : null;
  const log = task.log ?? [];
  const steps = log.filter((line) => describeActivity(line).kind === "tool");
  const lines = report ? steps : log.slice(-8);
  const time = elapsed(task, now);

  return (
    <div className="agent-net-panel">
      <div className="agent-net-panel-head">
        <span className="agent-net-panel-title">
          <span className="agent-net-panel-name">{task.agent}</span> {WORDS[task.status]}
        </span>
        <StepTally log={log} />
        {time && <span className="agent-net-panel-time">{time}</span>}
        {report && (
          <button
            className="icon-btn"
            onClick={() =>
              post({
                type: "openUntitled",
                content: report,
                lang: "markdown",
                title: `${task.agent} report`,
              })
            }
            title="Open report in a markdown tab"
            aria-label="Open report in a markdown tab"
          >
            <ExternalIcon />
          </button>
        )}
      </div>
      {task.task && <p className="agent-net-ask">{task.task}</p>}
      {task.status === "error" && task.error && <Failure error={task.error} />}
      {lines.length > 0 && <ActivityLog lines={lines} />}
      {report && <Markdown text={report} />}
    </div>
  );
}

/** What went wrong, and the way out of it on its own line: the fix is the part
 *  the reader acts on, so it does not trail off the end of the sentence. */
function Failure({ error }: { error: string }) {
  const { what, fix } = failure(error);
  return (
    <p className="agent-net-fail">
      <AlertIcon />
      <span className="agent-net-fail-text">
        {what}
        {fix && <span className="agent-net-fail-fix">{fix}</span>}
      </span>
    </p>
  );
}

/** The mark the agent wears: a turning loader while it works, the outcome in
 *  its place once it lands. The swap crossfades so the row does not blink. */
function StateGlyph({ status }: { status: AgentTaskState["status"] }) {
  const reduced = useReducedMotion();
  const still = { opacity: 1, scale: 1 };
  const away = reduced ? { opacity: 0 } : { opacity: 0, scale: 0.5 };

  return (
    <AnimatePresence initial={false}>
      <motion.span
        key={status}
        className="agent-avatar-glyph"
        initial={away}
        animate={reduced ? { opacity: 1 } : still}
        exit={reduced ? { opacity: 0, transition: INSTANT } : away}
        transition={reduced ? INSTANT : CELL}
      >
        {status === "done" ? <CheckIcon /> : status === "error" ? <XIcon /> : <SpinnerIcon />}
      </motion.span>
    </AnimatePresence>
  );
}

/** One sub-agent as a single row: who it is, how it is doing, and the one line
 *  worth reading right now. The ask, the steps and the report are a click away,
 *  so five agents stay five rows. */
function AgentNode({
  task,
  selected,
  onSelect,
  nodeRef,
}: {
  task: AgentTaskState;
  selected: boolean;
  onSelect: () => void;
  nodeRef?: (el: HTMLButtonElement | null) => void;
}) {
  const live = task.status === "running" ? task.log?.at(-1) : undefined;
  const current = live ? describeActivity(live) : undefined;
  const report = task.report?.trim() ? task.report : undefined;
  const settled =
    task.status === "error" && task.error
      ? failure(task.error).what
      : report
        ? gist(report)
        : undefined;

  return (
    <button
      ref={nodeRef}
      className="agent-node"
      data-status={task.status}
      data-selected={selected}
      onClick={onSelect}
      aria-expanded={selected}
      aria-label={`${task.agent} ${WORDS[task.status]}`}
      title={task.task}
    >
      <span className="agent-avatar">
        <StateGlyph status={task.status} />
      </span>
      <span className="agent-node-name">{task.agent}</span>
      <span className="agent-node-line">
        {current?.kind === "tool" ? (
          <>
            <span className="agent-node-verb">{current.verb}</span>
            {current.detail && <span className="agent-node-detail">{current.detail}</span>}
          </>
        ) : (
          <span className="agent-node-say">{current?.text ?? settled ?? task.task}</span>
        )}
      </span>
      <span className="agent-node-state">
        <ChevronIcon open={selected} />
      </span>
    </button>
  );
}
