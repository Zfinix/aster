import { useRef, useState } from "react";
import { useDismiss } from "../lib/dismiss";
import { FileIcon, PlusIcon, UploadIcon } from "./icons";

/**
 * The `+` beside the box: the two ways a file gets into a message. Both end as
 * a mention, so the menu is about where the file is, not what happens to it.
 */
export function AddMenu({
  onUpload,
  onMention,
}: {
  onUpload: () => void;
  onMention: () => void;
}) {
  const ref = useRef<HTMLDivElement>(null);
  const [open, setOpen] = useState(false);
  useDismiss(ref, () => setOpen(false));

  const pick = (run: () => void) => () => {
    setOpen(false);
    run();
  };

  return (
    <div className="add" ref={ref}>
      <button
        className="ghost foot-btn"
        onClick={() => setOpen(!open)}
        title="Add a file"
        aria-label="Add a file"
        aria-haspopup="menu"
        aria-expanded={open}
      >
        <PlusIcon />
      </button>
      {open && (
        <div className="add-menu" role="menu">
          <button className="add-row" role="menuitem" onClick={pick(onUpload)}>
            <UploadIcon />
            Upload from this computer
          </button>
          <button className="add-row" role="menuitem" onClick={pick(onMention)}>
            <FileIcon />
            Mention a file in this repo
          </button>
        </div>
      )}
    </div>
  );
}
