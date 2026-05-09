"use client";

import { type ReactNode } from "react";
import { Bar, BarChart, CartesianGrid, Cell, ResponsiveContainer, Tooltip, XAxis, YAxis } from "recharts";
import {
  MEMPHIS_AXIS_LINE,
  MEMPHIS_AXIS_TICK,
  MEMPHIS_BAR_RADIUS,
  MEMPHIS_GRID,
  MEMPHIS_PIE_SEQUENCE,
  MEMPHIS_PIE_STROKE,
  MEMPHIS_PIE_STROKE_WIDTH,
  colorForIndex,
  useChartAnimation,
} from "./theme";
import { MemphisTooltip } from "./MemphisTooltip";

export type MemphisBarDatum = {
  name: string;
  value: number;
  color?: string;
};

export type MemphisBarChartProps = {
  data: MemphisBarDatum[];
  orientation?: "vertical" | "horizontal";
  height?: number;
  ariaLabel: string;
  formatValueTick?: (value: number) => string;
  formatTooltipValue?: (value: number | string, key: string) => ReactNode;
  emptyState?: ReactNode;
};

export function MemphisBarChart({
  data,
  orientation = "vertical",
  height = 280,
  ariaLabel,
  formatValueTick,
  formatTooltipValue,
  emptyState,
}: MemphisBarChartProps) {
  const motion = useChartAnimation();
  if (!data.length) {
    return (
      <div role="img" aria-label={ariaLabel} className="flex items-center justify-center" style={{ height }}>
        {emptyState ?? null}
      </div>
    );
  }
  const colored = data.map((item, idx) => ({
    ...item,
    color: item.color ?? MEMPHIS_PIE_SEQUENCE[idx % MEMPHIS_PIE_SEQUENCE.length] ?? colorForIndex(idx),
  }));
  const isHorizontal = orientation === "horizontal";
  return (
    <figure role="img" aria-label={ariaLabel} className="w-full">
      <ResponsiveContainer width="100%" height={height}>
        <BarChart
          data={colored}
          layout={isHorizontal ? "vertical" : "horizontal"}
          margin={{ top: 12, right: 16, bottom: 8, left: isHorizontal ? 16 : 0 }}
          accessibilityLayer
        >
          <CartesianGrid stroke={MEMPHIS_GRID.stroke} strokeDasharray={MEMPHIS_GRID.strokeDasharray} strokeWidth={MEMPHIS_GRID.strokeWidth} vertical={isHorizontal} horizontal={!isHorizontal} />
          {isHorizontal ? (
            <>
              <XAxis type="number" tick={MEMPHIS_AXIS_TICK} axisLine={false} tickLine={false} tickMargin={8} tickFormatter={formatValueTick as ((value: unknown) => string) | undefined} />
              <YAxis type="category" dataKey="name" tick={MEMPHIS_AXIS_TICK} axisLine={MEMPHIS_AXIS_LINE} tickLine={false} tickMargin={8} width={100} />
            </>
          ) : (
            <>
              <XAxis type="category" dataKey="name" tick={MEMPHIS_AXIS_TICK} axisLine={MEMPHIS_AXIS_LINE} tickLine={false} tickMargin={8} />
              <YAxis type="number" tick={MEMPHIS_AXIS_TICK} axisLine={false} tickLine={false} tickMargin={8} width={56} tickFormatter={formatValueTick as ((value: unknown) => string) | undefined} />
            </>
          )}
          <Tooltip
            cursor={{ fill: "rgba(139, 92, 246, 0.08)" }}
            content={(props: object) => (
              <MemphisTooltip {...(props as Record<string, unknown>)} formatValue={formatTooltipValue} />
            )}
          />
          <Bar
            dataKey="value"
            radius={MEMPHIS_BAR_RADIUS}
            stroke={MEMPHIS_PIE_STROKE}
            strokeWidth={MEMPHIS_PIE_STROKE_WIDTH}
            {...motion}
          >
            {colored.map((entry, idx) => (
              <Cell key={`${entry.name}-${idx}`} fill={entry.color} />
            ))}
          </Bar>
        </BarChart>
      </ResponsiveContainer>
    </figure>
  );
}
