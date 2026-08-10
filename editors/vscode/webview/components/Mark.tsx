import { useState } from "react";

const N = 13;
const C = 6;
/** Top-to-bottom warm gradient of the mark; the one source of brand colour. */
export const ROW_COLORS = [
  "#ef5a6f", "#ef5a6f", "#f0664f", "#f2764f", "#f4864e", "#f79b4d", "#f79b4d",
  "#f6a854", "#f7b458", "#f7c15c", "#f7c15c", "#f8cb66", "#f8cb66",
];

const RECTS: { x: number; y: number; fill: string }[] = [];
for (let r = 0; r < N; r++) {
  for (let c = 0; c < N; c++) {
    const on =
      c === C ||
      r === C ||
      ((r === c || r + c === N - 1) && Math.abs(r - C) <= 4);
    if (on) RECTS.push({ x: c, y: r, fill: ROW_COLORS[r] });
  }
}

/** The pixel-asterisk Aster mark as inline SVG, same geometry as the desktop
    app's. `px` is the size of one cell, so the mark renders at `13 * px`
    square. With `interactive`, clicking it plays a single snappy spin. */
export function Mark({
  px = 3,
  label = "Aster",
  className = "",
  interactive = false,
}: {
  px?: number;
  label?: string;
  className?: string;
  interactive?: boolean;
}) {
  const size = N * px;
  const [spinning, setSpinning] = useState(false);
  return (
    <svg
      className={`mark ${className} ${interactive ? "mark-clickable" : ""} ${spinning ? "mark-spin" : ""}`}
      width={size}
      height={size}
      viewBox={`0 0 ${N} ${N}`}
      shapeRendering="crispEdges"
      role="img"
      aria-label={label}
      onClick={interactive ? () => setSpinning(true) : undefined}
      onAnimationEnd={interactive ? () => setSpinning(false) : undefined}
    >
      {RECTS.map((r, i) => (
        <rect key={i} x={r.x} y={r.y} width="1" height="1" fill={r.fill} />
      ))}
    </svg>
  );
}
