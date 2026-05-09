"use client";

import { useEffect, useState } from "react";

export const MEMPHIS_PALETTE = {
  ink: "#1E293B",
  paper: "#FFFDF5",
  mutedLine: "#E2E8F0",
  violet: "#8B5CF6",
  violetSoft: "#C4B5FD",
  pink: "#F472B6",
  pinkSoft: "#FBCFE8",
  amber: "#FBBF24",
  amberSoft: "#FDE68A",
  emerald: "#34D399",
  emeraldSoft: "#A7F3D0",
  slate: "#64748B",
} as const;

export const MEMPHIS_SERIES_COLORS = [
  MEMPHIS_PALETTE.violet,
  MEMPHIS_PALETTE.amber,
  MEMPHIS_PALETTE.pink,
  MEMPHIS_PALETTE.emerald,
] as const;

export const MEMPHIS_PIE_SEQUENCE = [
  MEMPHIS_PALETTE.violet,
  MEMPHIS_PALETTE.pink,
  MEMPHIS_PALETTE.amber,
  MEMPHIS_PALETTE.emerald,
  MEMPHIS_PALETTE.violetSoft,
  MEMPHIS_PALETTE.pinkSoft,
  MEMPHIS_PALETTE.amberSoft,
  MEMPHIS_PALETTE.slate,
] as const;

export const MEMPHIS_AXIS_TICK = {
  fontFamily: '"Plus Jakarta Sans", system-ui, sans-serif',
  fontSize: 12,
  fontWeight: 600,
  fill: MEMPHIS_PALETTE.ink,
} as const;

export const MEMPHIS_AXIS_LINE = {
  stroke: MEMPHIS_PALETTE.ink,
  strokeWidth: 2,
} as const;

export const MEMPHIS_GRID = {
  stroke: MEMPHIS_PALETTE.mutedLine,
  strokeDasharray: "4 4",
  strokeWidth: 1,
} as const;

export const MEMPHIS_LINE_STROKE_WIDTH = 3;
export const MEMPHIS_DOT_RADIUS = 5;
export const MEMPHIS_BAR_RADIUS: [number, number, number, number] = [10, 10, 0, 0];
export const MEMPHIS_PIE_STROKE = MEMPHIS_PALETTE.ink;
export const MEMPHIS_PIE_STROKE_WIDTH = 2;

export const MEMPHIS_ANIMATION_DURATION = 320;
export const MEMPHIS_ANIMATION_EASING: "ease-out" | "linear" = "ease-out";

export function colorForIndex(index: number): string {
  return MEMPHIS_PIE_SEQUENCE[index % MEMPHIS_PIE_SEQUENCE.length];
}

export function useReducedMotion(): boolean {
  const [reduced, setReduced] = useState(false);
  useEffect(() => {
    if (typeof window === "undefined" || !window.matchMedia) return;
    const query = window.matchMedia("(prefers-reduced-motion: reduce)");
    setReduced(query.matches);
    const onChange = (event: MediaQueryListEvent) => setReduced(event.matches);
    query.addEventListener("change", onChange);
    return () => query.removeEventListener("change", onChange);
  }, []);
  return reduced;
}

export function useChartAnimation() {
  const reducedMotion = useReducedMotion();
  return {
    isAnimationActive: !reducedMotion,
    animationDuration: reducedMotion ? 0 : MEMPHIS_ANIMATION_DURATION,
    animationEasing: MEMPHIS_ANIMATION_EASING,
  };
}
