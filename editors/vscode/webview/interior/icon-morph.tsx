import { motion, useReducedMotion } from "motion/react";
import { CELL, INSTANT } from "./springs";

/** Shapes must agree on slot count and points per slot, or the path snaps
 *  instead of morphing. */
export type MorphShape = {
  d: readonly string[];
  rotate?: number;
  fill?: boolean;
};

export const sendStop: readonly MorphShape[] = [
  { d: ["M 8 13 L 8 8 L 8 3", "M 3.5 7.5 L 8 3 L 12.5 7.5"] },
  // The two polylines close across the same diagonal, so filled they tile the
  // square solid.
  { d: ["M 5 5 L 11 5 L 11 11", "M 11 11 L 5 11 L 5 5"], fill: true },
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
        {/* The solid interior, faded in under the morphing outline. A rect of
            its own because path fill-opacity does not animate reliably. */}
        <motion.rect
          x="5"
          y="5"
          width="6"
          height="6"
          rx="1.2"
          fill="currentColor"
          stroke="none"
          initial={false}
          animate={{ opacity: shape.fill ? 1 : 0 }}
          transition={transition}
        />
        {shape.d.map((d, i) => (
          <motion.path key={i} initial={false} animate={{ d }} transition={transition} />
        ))}
      </svg>
    </motion.span>
  );
}
