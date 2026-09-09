import { useEffect, useMemo, useState } from "react";
import type { MemoryBlock, MemoryProject } from "../../src/protocol";
import { PROJECT, blockWhen, countFacts, filterBlocks, plural } from "../lib/memory";
import { useListNav } from "../lib/listnav";
import { SearchIcon } from "./icons";
import { MemoryRow, type MemoryBody } from "./MemoryRow";
import { Modal } from "./Modal";

const SEARCHABLE = 6;

/** What Aster remembers, as something you can read rather than a table of
 *  names: newest first, each row opening onto the fact itself. Project memory
 *  leads, since it is the file the agent reads at the start of every turn. */
export function MemoryPanel({
  blocks,
  project,
  error,
  bodies,
  onRead,
  onForget,
  onClose,
}: {
  blocks: MemoryBlock[];
  project: MemoryProject | null;
  error?: string;
  bodies: Record<string, MemoryBody>;
  onRead: (name: string) => void;
  onForget: (name: string) => void;
  onClose: () => void;
}) {
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<string>();

  const rows = useMemo(() => filterBlocks(blocks, query), [blocks, query]);
  const facts = useMemo(() => countFacts(project), [project]);

  // A body is read the first time its row is opened and kept after that, so
  // reopening a block does not blink.
  useEffect(() => {
    if (open && open !== PROJECT && !bodies[open]) {
      onRead(open);
    }
  }, [open, bodies, onRead]);

  const toggle = (name: string) => setOpen((prev) => (prev === name ? undefined : name));

  const { active: cursor, setActive: setCursor, leave, onKey, seat } = useListNav<HTMLDivElement>({
    count: rows.length,
    resetOn: query,
    onPick: (index) => toggle(rows[index].name),
  });

  return (
    <Modal label="Memory" className="memory-panel" onClose={onClose}>
      <div className="memory-keys" onKeyDown={onKey}>
        <div className="memory-head">
          <span className="memory-title">Memory</span>
          <span className="memory-count">
            {plural(blocks.length, "block")}
            {facts > 0 && ` · ${plural(facts, "fact")} in ${PROJECT}`}
          </span>
        </div>

        {blocks.length > SEARCHABLE && (
          <div className="memory-search">
            <SearchIcon />
            <input
              placeholder="Search memory…"
              value={query}
              onChange={(e) => setQuery(e.target.value)}
              autoFocus
              spellCheck={false}
              aria-label="Search memory"
            />
          </div>
        )}

        <div className="memory-list" onMouseLeave={leave}>
          {error && <div className="memory-empty">{error}</div>}

          {project && (
            <MemoryRow
              name={PROJECT}
              meta={plural(facts, "fact")}
              description="Facts about this project, read at the start of every turn."
              open={open === PROJECT}
              body={{ body: project.text }}
              onToggle={() => toggle(PROJECT)}
            />
          )}

          {rows.map((block, at) => (
            <MemoryRow
              key={block.name}
              seat={seat(at)}
              name={block.name}
              meta={blockWhen(block)}
              description={block.description}
              open={open === block.name}
              body={bodies[block.name]}
              cursor={at === cursor}
              onEnter={() => setCursor(at)}
              onToggle={() => toggle(block.name)}
              onForget={() => {
                setOpen(undefined);
                onForget(block.name);
              }}
            />
          ))}

          {!error && rows.length === 0 && (
            <div className="memory-empty">
              {blocks.length > 0
                ? `Nothing matches "${query}".`
                : project
                  ? "No named blocks yet. Aster writes one when it learns something worth keeping."
                  : "Nothing remembered yet. Aster saves facts as it learns them."}
            </div>
          )}
        </div>
      </div>
    </Modal>
  );
}
