import type { ReactNode } from "react";

type MoneyAmountProps = {
  gross?: string | number | null;
  fee?: string | number | null;
  net?: string | number | null;
  currency?: string;
  layout?: "row" | "stacked" | "inline";
};

function fmt(v: string | number | null | undefined) {
  if (v === null || v === undefined || v === "") return "0.00";
  return String(v);
}

export function MoneyAmount({ gross, fee, net, currency = "USD", layout = "row" }: MoneyAmountProps) {
  const showFee = fee !== undefined && fee !== null;
  const showNet = net !== undefined && net !== null;
  if (layout === "inline") {
    return (
      <span className="font-mono text-sm">
        ${fmt(gross)}
        {showFee && <span className="text-slate-400"> · 手续费 ${fmt(fee)}</span>}
        {showNet && <span className="font-bold"> · 实付 ${fmt(net)}</span>}
      </span>
    );
  }
  if (layout === "stacked") {
    return (
      <div className="grid gap-1">
        <Row label="金额" value={fmt(gross)} currency={currency} strong />
        {showFee && <Row label="手续费" value={fmt(fee)} currency={currency} muted />}
        {showNet && <Row label="实际" value={fmt(net)} currency={currency} highlight />}
      </div>
    );
  }
  return (
    <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
      <Cell label="金额" value={fmt(gross)} currency={currency} strong />
      {showFee && <Cell label="手续费" value={fmt(fee)} currency={currency} muted />}
      {showNet && <Cell label="实际" value={fmt(net)} currency={currency} highlight />}
    </div>
  );
}

function Cell({ label, value, currency, strong, muted, highlight }: { label: string; value: string; currency: string; strong?: boolean; muted?: boolean; highlight?: boolean }) {
  return (
    <div className="flex flex-col">
      <span className="text-[10px] font-bold uppercase tracking-wider text-slate-500">{label}</span>
      <span className={`font-mono ${strong ? "text-base font-bold" : "text-sm"} ${muted ? "text-slate-500" : ""} ${highlight ? "text-violet-600 font-bold" : ""}`}>
        ${value} <span className="text-xs font-normal text-slate-400">{currency}</span>
      </span>
    </div>
  );
}

function Row(props: { label: string; value: string; currency: string; strong?: boolean; muted?: boolean; highlight?: boolean }) {
  return (
    <div className="flex items-center justify-between text-sm">
      <span className="text-slate-500">{props.label}</span>
      <span className={`font-mono ${props.strong ? "font-bold" : ""} ${props.muted ? "text-slate-500" : ""} ${props.highlight ? "text-violet-600 font-bold" : ""}`}>
        ${props.value} <span className="text-xs text-slate-400">{props.currency}</span>
      </span>
    </div>
  );
}

export function FormatUsd({ value }: { value: string | number | null | undefined }): ReactNode {
  return <span className="font-mono">${fmt(value)}</span>;
}
