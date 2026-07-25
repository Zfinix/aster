import { useEffect, useRef, useState } from "react";
import { Button, Input } from "@heroui/react";
import { modelShort } from "../lib/session";

/** Model picker: a name-only list of vetted models, a checkmark on the current
    one, and a "+" row to type any custom model id. */
export function ModelMenu({
  model,
  models,
  onModel,
  onAddModel,
  direction = "up",
}: {
  model: string;
  models: string[];
  onModel: (value: string) => void;
  onAddModel: (value: string) => void;
  direction?: "up" | "down";
}) {
  const [open, setOpen] = useState(false);
  const [adding, setAdding] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);
  const inputRef = useRef<HTMLInputElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) {
        setOpen(false);
        setAdding(false);
      }
    };
    const onKey = (e: KeyboardEvent) => {
      if (e.key === "Escape") {
        setOpen(false);
        setAdding(false);
      }
    };
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  useEffect(() => {
    if (adding) inputRef.current?.focus();
  }, [adding]);

  const commitAdd = () => {
    const value = inputRef.current?.value.trim();
    if (value) onAddModel(value);
    setAdding(false);
    setOpen(false);
  };

  return (
    <div className="dd-wrap" ref={wrapRef}>
      <Button
        aria-haspopup="menu"
        aria-expanded={open}
        onPress={() => setOpen((o) => !o)}
      >
        <span className="pill model-pill">
          <span>{modelShort(model)}</span>
        </span>
      </Button>
      {open && (
        <div className={`dd ${direction}`} role="menu">
          {models.map((m) => (
            <Button
              key={m}
              data-active={m === model}
              onPress={() => {
                onModel(m);
                setOpen(false);
              }}
              render={(props) => (
                <button
                  {...props}
                  role="menuitemradio"
                  aria-checked={m === model}
                  title={m}
                />
              )}
            >
              <span>{modelShort(m)}</span>
            </Button>
          ))}
          {adding ? (
            <Input
              ref={inputRef}
              className="dd-input"
              spellCheck={false}
              placeholder="provider/model-name"
              onKeyDown={(e) => {
                if (e.key === "Enter") commitAdd();
              }}
              onBlur={commitAdd}
            />
          ) : (
            <Button
              className="dd-add"
              onPress={() => setAdding(true)}
            >
              <span>+ Add model</span>
            </Button>
          )}
        </div>
      )}
    </div>
  );
}
