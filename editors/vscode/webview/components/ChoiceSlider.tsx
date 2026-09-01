import { useRef } from "react";

export interface SliderOption {
  value: string;
  label: string;
}

/** An ordered ladder as one dot per step: a dial you slide, not a list you
 *  read. The top step keeps its own color so the far end is legible at a
 *  glance. */
export function ChoiceSlider({
  label,
  options,
  value,
  onSelect,
}: {
  label: string;
  options: SliderOption[];
  value: string;
  onSelect: (value: string) => void;
}) {
  const at = Math.max(
    0,
    options.findIndex((option) => option.value === value)
  );

  const ref = useRef<HTMLDivElement>(null);

  const move = (delta: number) => {
    const to = Math.min(options.length - 1, Math.max(0, at + delta));
    const next = options[to];
    if (!next || next.value === value) return;
    onSelect(next.value);
    // The ring follows the value, so the dot being read is the dot in force.
    const dots = ref.current?.children;
    (dots?.[to] as HTMLElement | undefined)?.focus();
  };

  return (
    <div
      ref={ref}
      className="slider"
      role="radiogroup"
      aria-label={label}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
          e.preventDefault();
          move(-1);
        } else if (e.key === "ArrowRight" || e.key === "ArrowUp") {
          e.preventDefault();
          move(1);
        }
      }}
    >
      {options.map((option, index) => (
        <button
          key={option.value}
          className="slider-dot"
          role="radio"
          aria-checked={index === at}
          aria-label={option.label}
          title={option.label}
          data-on={index === at}
          data-top={index === options.length - 1}
          tabIndex={index === at ? 0 : -1}
          // Mousedown, not click: the box being typed into must keep focus so
          // the arrow keys still drive the menu around this dial.
          onMouseDown={(e) => {
            e.preventDefault();
            onSelect(option.value);
          }}
        />
      ))}
    </div>
  );
}
