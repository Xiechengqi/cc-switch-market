"use client";

import type { ReactNode } from "react";
import { useLocale } from "@/components/language-provider";

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
  const { locale } = useLocale();
  const labels = locale === "zh"
    ? { amount: "金额", fee: "手续费", net: "实际" }
    : { amount: "Amount", fee: "Fee", net: "Net" };
  const showFee = fee !== undefined && fee !== null;
  const showNet = net !== undefined && net !== null;
  if (layout === "inline") {
    return (
      <span className="font-mono text-sm">
        ${fmt(gross)}
        {showFee && <span className="text-slate-400"> · {labels.fee} ${fmt(fee)}</span>}
        {showNet && <span className="font-bold"> · {labels.net} ${fmt(net)}</span>}
      </span>
    );
  }
  if (layout === "stacked") {
    return (
      <div className="grid gap-1">
        <Row label={labels.amount} value={fmt(gross)} currency={currency} strong />
        {showFee && <Row label={labels.fee} value={fmt(fee)} currency={currency} muted />}
        {showNet && <Row label={labels.net} value={fmt(net)} currency={currency} highlight />}
      </div>
    );
  }
  return (
    <div className="flex flex-wrap items-baseline gap-x-4 gap-y-1">
      <Cell label={labels.amount} value={fmt(gross)} currency={currency} strong />
      {showFee && <Cell label={labels.fee} value={fmt(fee)} currency={currency} muted />}
      {showNet && <Cell label={labels.net} value={fmt(net)} currency={currency} highlight />}
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
