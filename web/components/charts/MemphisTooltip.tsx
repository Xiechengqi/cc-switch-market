"use client";

import type { ReactNode } from "react";

export type TooltipPayloadItem = {
  name?: string | number;
  value?: number | string;
  dataKey?: string | number;
  color?: string;
  payload?: Record<string, unknown>;
};

export type MemphisTooltipProps = {
  active?: boolean;
  label?: string | number;
  payload?: TooltipPayloadItem[];
  formatLabel?: (label: string | number | undefined) => ReactNode;
  formatSeriesLabel?: (key: string) => string;
  formatValue?: (value: number | string, key: string) => ReactNode;
};

export function MemphisTooltip({
  active,
  label,
  payload,
  formatLabel,
  formatSeriesLabel,
  formatValue,
}: MemphisTooltipProps) {
  if (!active || !payload || payload.length === 0) return null;
  return (
    <div className="rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 text-sm">
      {label !== undefined && (
        <div className="mb-1.5 text-xs font-extrabold uppercase tracking-wider text-slate-500">
          {formatLabel ? formatLabel(label) : label}
        </div>
      )}
      <ul className="grid gap-1">
        {payload.map((item, idx) => {
          const key = String(item.dataKey ?? item.name ?? idx);
          const seriesLabel = formatSeriesLabel ? formatSeriesLabel(key) : (item.name ?? key);
          const value = item.value ?? "";
          return (
            <li key={`${key}-${idx}`} className="flex items-center gap-2">
              <span
                aria-hidden
                className="inline-block h-3 w-3 shrink-0 rounded-full border-2 border-slate-800"
                style={{ backgroundColor: item.color }}
              />
              <span className="font-bold text-slate-800">{seriesLabel}</span>
              <span className="ml-auto font-mono font-extrabold text-slate-900">
                {formatValue ? formatValue(value, key) : value}
              </span>
            </li>
          );
        })}
      </ul>
    </div>
  );
}
