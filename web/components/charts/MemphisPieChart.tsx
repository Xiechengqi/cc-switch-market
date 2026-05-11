"use client";

import { type ReactNode } from "react";
import { Cell, Pie, PieChart, ResponsiveContainer, Tooltip } from "recharts";
import {
  MEMPHIS_PIE_SEQUENCE,
  MEMPHIS_PIE_STROKE,
  MEMPHIS_PIE_STROKE_WIDTH,
  colorForIndex,
  useChartAnimation,
} from "./theme";
import { MemphisTooltip } from "./MemphisTooltip";

export type MemphisPieDatum = {
  name: string;
  value: number;
  color?: string;
};

export type MemphisPieChartProps = {
  data: MemphisPieDatum[];
  height?: number;
  ariaLabel: string;
  innerRadiusRatio?: number;
  showLegend?: boolean;
  formatValue?: (value: number | string, key: string) => ReactNode;
  emptyState?: ReactNode;
};

export function MemphisPieChart({
  data,
  height = 260,
  ariaLabel,
  innerRadiusRatio = 0.55,
  showLegend = true,
  formatValue,
  emptyState,
}: MemphisPieChartProps) {
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
  const outerRadius = Math.max(48, Math.floor(height / 2 - 12));
  const innerRadius = Math.floor(outerRadius * innerRadiusRatio);
  return (
    <figure role="img" aria-label={ariaLabel} className="w-full">
      <div style={{ height }}>
        <ResponsiveContainer width="100%" height="100%">
          <PieChart accessibilityLayer>
            <Tooltip
              content={(props: object) => (
                <MemphisTooltip
                  {...(props as Record<string, unknown>)}
                  formatValue={formatValue}
                />
              )}
            />
            <Pie
              data={colored}
              dataKey="value"
              nameKey="name"
              innerRadius={innerRadius}
              outerRadius={outerRadius}
              stroke={MEMPHIS_PIE_STROKE}
              strokeWidth={MEMPHIS_PIE_STROKE_WIDTH}
              paddingAngle={2}
              {...motion}
            >
              {colored.map((entry, idx) => (
                <Cell key={`${entry.name}-${idx}`} fill={entry.color} />
              ))}
            </Pie>
          </PieChart>
        </ResponsiveContainer>
      </div>
      {showLegend && (
        <ul className="mt-3 grid gap-x-3 gap-y-1 grid-cols-1 sm:grid-cols-2 text-xs">
          {colored.map((item, idx) => (
            <li
              key={`${item.name}-${idx}`}
              className="flex min-w-0 items-center gap-2 font-bold text-slate-700"
            >
              <span
                aria-hidden
                className="h-2.5 w-2.5 shrink-0 rounded-full border-2 border-slate-800"
                style={{ backgroundColor: item.color }}
              />
              <span className="truncate" title={item.name}>{item.name}</span>
            </li>
          ))}
        </ul>
      )}
    </figure>
  );
}
