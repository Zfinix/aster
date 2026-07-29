import { FileIcon } from "./icons";

interface Props {
  items: string[];
  activeIndex: number;
  onSelect: (path: string) => void;
  onHover: (index: number) => void;
}

/** File suggestions for an @-mention in the composer, anchored above it. */
export function MentionMenu({ items, activeIndex, onSelect, onHover }: Props) {
  if (items.length === 0) return null;

  return (
    <div className="mention-menu" role="listbox" aria-label="File suggestions">
      {items.map((path, i) => {
        const slash = path.lastIndexOf("/");
        const name = slash === -1 ? path : path.slice(slash + 1);
        const dir = slash === -1 ? "" : path.slice(0, slash);
        return (
          <button
            key={path}
            type="button"
            role="option"
            aria-selected={i === activeIndex}
            data-active={i === activeIndex}
            onMouseDown={(e) => e.preventDefault()}
            onMouseEnter={() => onHover(i)}
            onClick={() => onSelect(path)}
          >
            <FileIcon />
            <span className="m-name">{name}</span>
            {dir && <span className="m-dir">{dir}</span>}
          </button>
        );
      })}
    </div>
  );
}
