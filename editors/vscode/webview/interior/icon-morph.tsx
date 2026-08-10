import { motion, useReducedMotion } from "motion/react";
import { CELL, INSTANT } from "./springs";

/** Shapes must agree on slot count and points per slot, or the path snaps
 *  instead of morphing. */
export type MorphShape = {
  d: readonly string[];
  rotate?: number;
};

/** Send and stop as two three-point polylines each, so one morphs into the
 *  other point for point. */
export const sendStop: readonly MorphShape[] = [
  { d: ["M 8 13 L 8 8 L 8 3", "M 3.5 7.5 L 8 3 L 12.5 7.5"] },
  { d: ["M 5 5 L 11 5 L 11 11", "M 11 11 L 5 11 L 5 5"] },
];

/** interior.dev's icon morph, reduced to the glyph: the caller owns the button
 *  so the panel's own chrome and handlers stay in charge. */
export function IconMorphGlyph({
  shapes,
  active,
  size = 14,
  strokeWidth = 1.4,
}: {
  shapes: readonly MorphShape[];
  active: number;
  size?: number;
  strokeWidth?: number;
}) {
  const reduced = useReducedMotion();
  const transition = reduced ? INSTANT : CELL;
  const shape = shapes[Math.min(Math.max(active, 0), shapes.length - 1)];

  return (
    <motion.span
      aria-hidden="true"
      className="stack"
      initial={false}
      animate={{ rotate: shape.rotate ?? 0 }}
      transition={transition}
      style={{ width: size, height: size }}
    >
      <svg
        viewBox="0 0 16 16"
        width={size}
        height={size}
        focusable="false"
        fill="none"
        stroke="currentColor"
        strokeWidth={strokeWidth}
        strokeLinecap="round"
        strokeLinejoin="round"
        style={{ display: "block" }}
      >
        {shape.d.map((d, i) => (
          <motion.path key={i} initial={false} animate={{ d }} transition={transition} />
        ))}
      </svg>
    </motion.span>
  );
}
