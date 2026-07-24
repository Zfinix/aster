import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useRef,
  useState,
  type ReactNode,
} from "react";

/* ---- Theme ---- */

export type Theme = "dark" | "light";

export function useTheme(): [Theme, (t: Theme) => void] {
  const [theme, setThemeState] = useState<Theme>(
    () =>
      (document.documentElement.dataset.theme as Theme | undefined) ?? "dark",
  );
  const setTheme = useCallback((t: Theme) => {
    document.documentElement.dataset.theme = t;
    setThemeState(t);
  }, []);
  return [theme, setTheme];
}

/* ---- Toast ---- */

const ToastCtx = createContext<(message: string) => void>(() => {});
export const useToast = () => useContext(ToastCtx);

export function ToastProvider({ children }: { children: ReactNode }) {
  const [msg, setMsg] = useState("");
  const [show, setShow] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

  const toast = useCallback((message: string) => {
    setMsg(message);
    setShow(true);
    if (timer.current) clearTimeout(timer.current);
    timer.current = setTimeout(() => setShow(false), 2200);
  }, []);

  useEffect(() => () => void (timer.current && clearTimeout(timer.current)), []);

  return (
    <ToastCtx.Provider value={toast}>
      {children}
      <div className={`app-toast ${show ? "show" : ""}`} role="status">
        {msg}
      </div>
    </ToastCtx.Provider>
  );
}

/* ---- Dropdown ---- */

export interface DropdownOption {
  value: string;
  label: ReactNode;
  hint?: string;
}

/** A small, outside-click-aware menu anchored to a trigger. */
export function Dropdown({
  trigger,
  options,
  value,
  onSelect,
  direction = "up",
  align = "left",
}: {
  trigger: (open: boolean) => ReactNode;
  options: DropdownOption[];
  value?: string;
  onSelect: (value: string) => void;
  direction?: "up" | "down";
  align?: "left" | "right";
}) {
  const [open, setOpen] = useState(false);
  const wrapRef = useRef<HTMLDivElement>(null);

  useEffect(() => {
    if (!open) return;
    const onDoc = (e: MouseEvent) => {
      if (!wrapRef.current?.contains(e.target as Node)) setOpen(false);
    };
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && setOpen(false);
    document.addEventListener("mousedown", onDoc);
    document.addEventListener("keydown", onKey);
    return () => {
      document.removeEventListener("mousedown", onDoc);
      document.removeEventListener("keydown", onKey);
    };
  }, [open]);

  return (
    <div className="dd-wrap" ref={wrapRef}>
      <button
        type="button"
        aria-haspopup="menu"
        aria-expanded={open}
        onClick={() => setOpen((o) => !o)}
      >
        {trigger(open)}
      </button>
      {open && (
        <div
          className={`dd ${direction}`}
          role="menu"
          style={align === "right" ? { left: "auto", right: 0 } : undefined}
        >
          {options.map((o) => (
            <button
              key={o.value}
              type="button"
              role="menuitem"
              data-active={value === o.value}
              onClick={() => {
                onSelect(o.value);
                setOpen(false);
              }}
            >
              <span>{o.label}</span>
              {o.hint && <small>{o.hint}</small>}
            </button>
          ))}
        </div>
      )}
    </div>
  );
}
