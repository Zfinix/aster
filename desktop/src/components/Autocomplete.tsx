import { FileIcon, FolderIcon } from "./icons";

export interface Suggestion {
  value: string;
  label: string;
  detail?: string;
  full?: string;
  dir?: boolean;
}

export function Autocomplete({
  items,
  active,
  onPick,
}: {
  items: Suggestion[];
  active: number;
  onPick: (item: Suggestion) => void;
}) {
  if (items.length === 0) {
    return null;
  }
  return (
    <div className="pop">
      <div className="picker" role="listbox">
      {items.map((item, i) => (
        <button
          key={item.value}
          className="picker-row"
          role="option"
          aria-selected={i === active}
          data-active={i === active}
          // Mousedown, not click: the textarea must not lose focus first.
          onMouseDown={(e) => {
            e.preventDefault();
            onPick(item);
          }}
        >
          {item.dir ? <FolderIcon /> : <FileIcon />}
          <span className="picker-body">
            <span className="picker-label">{item.label}</span>
            {item.detail && <span className="picker-detail">{item.detail}</span>}
          </span>
        </button>
      ))}
      </div>
    </div>
  );
}
