import { useLayoutEffect, useRef, type KeyboardEvent } from "react";

export interface SegmentOption {
  value: string;
  label: string;
  danger?: boolean;
  disabled?: boolean;
  title?: string;
}

/** The selected pill is a clipped copy of the whole row rather than a box
 *  that moves under the labels: sliding the clip carries the label colours
 *  with it, so a change reads as one motion instead of a swap. */
export function Segmented({
  options,
  value,
  label,
  onChange,
}: {
  options: SegmentOption[];
  value: string;
  label: string;
  onChange: (next: string) => void;
}) {
  const root = useRef<HTMLDivElement>(null);
  const overlay = useRef<HTMLDivElement>(null);
  const current = options.find((option) => option.value === value);
  const focusable = current?.value ?? options[0]?.value;

  useLayoutEffect(() => {
    const el = root.current;
    const on = overlay.current;
    if (!el || !on) return;
    const measure = () => {
      const active = el.querySelector<HTMLElement>('.set-segment[aria-checked="true"]');
      if (!active) {
        on.style.clipPath = "inset(0 100% 0 0)";
        return;
      }
      const right = el.clientWidth - active.offsetLeft - active.offsetWidth;
      const bottom = el.clientHeight - active.offsetTop - active.offsetHeight;
      on.style.clipPath = `inset(${active.offsetTop}px ${right}px ${bottom}px ${active.offsetLeft}px round 6px)`;
    };
    measure();
    const watch = new ResizeObserver(measure);
    watch.observe(el);
    return () => watch.disconnect();
  }, [value, options]);

  const onKeyDown = (e: KeyboardEvent<HTMLDivElement>) => {
    const step =
      e.key === "ArrowRight" || e.key === "ArrowDown"
        ? 1
        : e.key === "ArrowLeft" || e.key === "ArrowUp"
          ? -1
          : 0;
    if (!step) return;
    e.preventDefault();
    const enabled = options.filter((option) => !option.disabled);
    const at = enabled.findIndex((option) => option.value === value);
    const next = enabled[(at + step + enabled.length) % enabled.length];
    if (!next || next.value === value) return;
    onChange(next.value);
    root.current
      ?.querySelector<HTMLElement>(`.set-segment[data-value="${CSS.escape(next.value)}"]`)
      ?.focus();
  };

  return (
    <div
      ref={root}
      className="set-segmented"
      role="radiogroup"
      aria-label={label}
      data-danger={current?.danger === true}
      onKeyDown={onKeyDown}
    >
      {options.map((option) => (
        <button
          type="button"
          key={option.value}
          role="radio"
          aria-checked={option.value === value}
          data-value={option.value}
          tabIndex={option.value === focusable ? 0 : -1}
          className="set-segment"
          disabled={option.disabled}
          title={option.title}
          onClick={() => option.value !== value && onChange(option.value)}
        >
          {option.label}
        </button>
      ))}
      <div ref={overlay} className="set-segmented-on" aria-hidden="true">
        {options.map((option) => (
          <span key={option.value} className="set-segment">
            {option.label}
          </span>
        ))}
      </div>
    </div>
  );
}
