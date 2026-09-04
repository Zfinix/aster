import { useRef } from "react";

export interface SliderOption {
  value: string;
  label: string;
}

/** One stop per option, in px; the knob's travel is a multiple of it. */
const STEP = 13;

/** An ordered ladder as a knob on a track: drag it, tap a stop, or arrow it
 *  along. The top stop keeps its own colour so the far end is legible at a
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
  const dragging = useRef(false);

  const pick = (to: number) => {
    const next = options[Math.min(options.length - 1, Math.max(0, to))];
    if (next && next.value !== value) onSelect(next.value);
  };

  /** The stop nearest the pointer, so a drag snaps as it goes. */
  const stopAt = (clientX: number) => {
    const track = ref.current;
    if (!track) return at;
    const first = track.querySelector<HTMLElement>(".slider-stop");
    const left = first?.getBoundingClientRect().left ?? track.getBoundingClientRect().left;
    return Math.round((clientX - left - STEP / 2) / STEP);
  };

  return (
    <div
      ref={ref}
      className="slider"
      role="radiogroup"
      aria-label={label}
      // Pointer, not mouse: one path for a drag from a mouse, a trackpad or a
      // finger, and capture keeps it following past the track's edge.
      onPointerDown={(e) => {
        // The box being typed into keeps focus, so the arrow keys still drive
        // the menu around this dial.
        e.preventDefault();
        e.currentTarget.setPointerCapture(e.pointerId);
        dragging.current = true;
        pick(stopAt(e.clientX));
      }}
      onPointerMove={(e) => {
        if (dragging.current) pick(stopAt(e.clientX));
      }}
      onPointerUp={() => {
        dragging.current = false;
      }}
      onPointerCancel={() => {
        dragging.current = false;
      }}
      onKeyDown={(e) => {
        if (e.key === "ArrowLeft" || e.key === "ArrowDown") {
          e.preventDefault();
          pick(at - 1);
        } else if (e.key === "ArrowRight" || e.key === "ArrowUp") {
          e.preventDefault();
          pick(at + 1);
        }
      }}
    >
      {options.map((option, index) => (
        <button
          key={option.value}
          className="slider-stop"
          role="radio"
          aria-checked={index === at}
          aria-label={option.label}
          title={option.label}
          data-on={index === at}
          data-top={index === options.length - 1}
          tabIndex={index === at ? 0 : -1}
        />
      ))}
      <span
        className="slider-knob"
        data-top={at === options.length - 1}
        style={{ "--at": at } as React.CSSProperties}
      />
    </div>
  );
}
