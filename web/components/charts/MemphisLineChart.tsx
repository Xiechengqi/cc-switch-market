"use client";

import { type ReactNode } from "react";
import {
  CartesianGrid,
  Legend,
  Line,
  LineChart,
  ResponsiveContainer,
  Tooltip,
  XAxis,
  YAxis,
} from "recharts";
import {
  MEMPHIS_AXIS_LINE,
  MEMPHIS_AXIS_TICK,
  MEMPHIS_DOT_RADIUS,
  MEMPHIS_GRID,
  MEMPHIS_LINE_STROKE_WIDTH,
  MEMPHIS_SERIES_COLORS,
  colorForIndex,
  useChartAnimation,
} from "./theme";
import { MemphisTooltip } from "./MemphisTooltip";

export type MemphisLineSeries = {
  key: string;
  label: string;
  color?: string;
};

export type MemphisLineChartProps = {
  data: Array<Record<string, number | string>>;
  xKey: string;
  series: MemphisLineSeries[];
  height?: number;
  ariaLabel: string;
  formatXTick?: (value: string | number) => string;
  formatYTick?: (value: number) => string;
  formatTooltipLabel?: (label: string | number | undefined) => ReactNode;
  formatTooltipValue?: (value: number | string, key: string) => ReactNode;
  showLegend?: boolean;
  emptyState?: ReactNode;
};

export function MemphisLineChart({
  data,
  xKey,
  series,
  height = 280,
  ariaLabel,
  formatXTick,
  formatYTick,
  formatTooltipLabel,
  formatTooltipValue,
  showLegend = true,
  emptyState,
}: MemphisLineChartProps) {
  const motion = useChartAnimation();
  if (!data.length || !series.length) {
    return (
      <div role="img" aria-label={ariaLabel} className="flex items-center justify-center" style={{ height }}>
        {emptyState ?? null}
      </div>
    );
  }
  const seriesLabelMap = Object.fromEntries(series.map((item) => [item.key, item.label]));
  return (
    <figure role="img" aria-label={ariaLabel} className="w-full">
      <ResponsiveContainer width="100%" height={height}>
        <LineChart data={data} margin={{ top: 12, right: 16, bottom: 8, left: 0 }} accessibilityLayer>
          <CartesianGrid stroke={MEMPHIS_GRID.stroke} strokeDasharray={MEMPHIS_GRID.strokeDasharray} strokeWidth={MEMPHIS_GRID.strokeWidth} vertical={false} />
          <XAxis
            dataKey={xKey}
            tick={MEMPHIS_AXIS_TICK}
            axisLine={MEMPHIS_AXIS_LINE}
            tickLine={false}
            tickMargin={8}
            tickFormatter={formatXTick as ((value: unknown) => string) | undefined}
          />
          <YAxis
            tick={MEMPHIS_AXIS_TICK}
            axisLine={false}
            tickLine={false}
            tickMargin={8}
            width={56}
            tickFormatter={formatYTick as ((value: unknown) => string) | undefined}
          />
          <Tooltip
            cursor={{ stroke: MEMPHIS_AXIS_LINE.stroke, strokeWidth: 1, strokeDasharray: "3 3" }}
            content={(props: object) => (
              <MemphisTooltip
                {...(props as Record<string, unknown>)}
                formatLabel={formatTooltipLabel}
                formatSeriesLabel={(key) => seriesLabelMap[key] ?? key}
                formatValue={formatTooltipValue}
              />
            )}
          />
          {showLegend && (
            <Legend
              verticalAlign="top"
              align="right"
              iconType="circle"
              iconSize={10}
              wrapperStyle={{ fontFamily: MEMPHIS_AXIS_TICK.fontFamily, fontSize: 12, fontWeight: 700 }}
              formatter={(value: string) => seriesLabelMap[value] ?? value}
            />
          )}
          {series.map((item, idx) => (
            <Line
              key={item.key}
              type="monotone"
              dataKey={item.key}
              stroke={item.color ?? MEMPHIS_SERIES_COLORS[idx % MEMPHIS_SERIES_COLORS.length] ?? colorForIndex(idx)}
              strokeWidth={MEMPHIS_LINE_STROKE_WIDTH}
              strokeLinecap="round"
              dot={false}
              activeDot={{ r: MEMPHIS_DOT_RADIUS, strokeWidth: 2, stroke: "#1E293B" }}
              {...motion}
            />
          ))}
        </LineChart>
      </ResponsiveContainer>
    </figure>
  );
}
