export const EASE = [0.23, 1, 0.32, 1] as const;
export const CELL = { type: "spring", stiffness: 520, damping: 34, mass: 0.45 } as const;
export const CROSSFADE = { type: "spring", stiffness: 260, damping: 34, mass: 0.8 } as const;
export const ARRIVE = { type: "spring", stiffness: 540, damping: 34, mass: 0.5 } as const;
export const POP = { type: "spring", stiffness: 640, damping: 22, mass: 0.7 } as const;
export const DISCLOSE = { type: "spring", stiffness: 190, damping: 30, mass: 1 } as const;
export const DRAW = { duration: 0.26, ease: EASE } as const;
export const INSTANT = { duration: 0 } as const;
