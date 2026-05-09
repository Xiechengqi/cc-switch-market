"use client";

import Link from "next/link";
import { useCallback, useEffect, useState } from "react";
import dynamic from "next/dynamic";
import { ArrowRight, Coins, Cpu, Layers, RefreshCw, ShieldAlert, Sparkles, Users } from "lucide-react";
import { apiGet } from "@/lib/client-api";
import { copy } from "@/lib/copy";
import { useLocale } from "@/components/language-provider";
import { Skeleton } from "@/components/ui/Skeleton";
import { EmptyState } from "@/components/ui/EmptyState";
import { DataTable, type Column } from "@/components/ui/DataTable";
import { Confetti } from "@/components/ui/Confetti";

const MemphisLineChart = dynamic(() => import("@/components/charts/MemphisLineChart").then((m) => m.MemphisLineChart), { ssr: false, loading: () => <Skeleton className="h-72 w-full rounded-2xl" /> });
const MemphisPieChart = dynamic(() => import("@/components/charts/MemphisPieChart").then((m) => m.MemphisPieChart), { ssr: false, loading: () => <Skeleton className="h-64 w-full rounded-2xl" /> });

type WindowKey = "24h" | "7d" | "30d";

type KpisResponse = {
  windowKey: string;
  windowDays: number;
  totalSpendUsd: string;
  totalRequests: number;
  totalTopupUsd: string;
  totalTopupOrders: number;
  activeApiUsers: number;
  activeProviders: number;
  registeredUsers: number;
  onlineShares: number;
};

type TrendResponse = {
  days: number;
  series: Array<{ date: string; spendUsd: string; topupUsd: string; requests: number }>;
};

type BreakdownBucket = { name: string; spendUsd: string; requests: number };
type BreakdownResponse = { dim: string; days: number; buckets: BreakdownBucket[] };

type ModelRow = { appType: string; model: string; spendUsd: string; requests: number; uniqueUsers: number };
type ProviderRow = { ownerEmail: string; grossSpendUsd: string; requests: number; uniqueShares: number };
type UserRow = { email: string; spendUsd: string; requests: number };

const TREND_DAYS = 30;
const BREAKDOWN_DAYS = 30;
const TOP_LIMIT = 10;

function formatMoney(value: string | number | undefined | null): string {
  if (value === null || value === undefined || value === "") return "$0";
  const num = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(num)) return "$0";
  if (num >= 1_000_000) return `$${(num / 1_000_000).toFixed(2)}M`;
  if (num >= 10_000) return `$${(num / 1_000).toFixed(1)}K`;
  if (num >= 100) return `$${num.toFixed(0)}`;
  if (num >= 1) return `$${num.toFixed(2)}`;
  return `$${num.toFixed(4).replace(/0+$/, "").replace(/\.$/, "")}`;
}

function formatMoneyFull(value: string | number | undefined | null): string {
  if (value === null || value === undefined || value === "") return "$0.00";
  const num = typeof value === "number" ? value : Number(value);
  if (!Number.isFinite(num)) return "$0.00";
  return `$${num.toLocaleString(undefined, { minimumFractionDigits: 2, maximumFractionDigits: 6 })}`;
}

function formatInt(value: number | string | undefined | null): string {
  const num = typeof value === "number" ? value : Number(value ?? 0);
  if (!Number.isFinite(num)) return "0";
  return num.toLocaleString();
}

export function BoardRoot() {
  const { locale } = useLocale();
  const c = copy[locale].board;
  const [windowKey, setWindowKey] = useState<WindowKey>("7d");
  const [refreshTick, setRefreshTick] = useState(0);
  const [refreshing, setRefreshing] = useState(false);
  const [lastUpdated, setLastUpdated] = useState<Date | null>(null);

  const handleRefresh = useCallback(() => {
    setRefreshing(true);
    setRefreshTick((tick) => tick + 1);
    window.setTimeout(() => setRefreshing(false), 600);
  }, []);

  return (
    <>
      <BoardHero c={c} windowKey={windowKey} onWindowChange={setWindowKey} onRefresh={handleRefresh} refreshing={refreshing} lastUpdated={lastUpdated} />
      <section className="mx-auto max-w-6xl px-6 pb-10">
        <KpiRow c={c} windowKey={windowKey} refreshTick={refreshTick} onUpdated={setLastUpdated} />
      </section>
      <section className="mx-auto max-w-6xl bg-dot-grid-soft px-6 py-12">
        <TrendCard c={c} refreshTick={refreshTick} />
      </section>
      <section className="mx-auto max-w-6xl px-6 py-12">
        <BreakdownGrid c={c} refreshTick={refreshTick} />
      </section>
      <section className="mx-auto max-w-6xl bg-stripes-soft px-6 py-12">
        <TopModelsCard c={c} windowKey={windowKey} refreshTick={refreshTick} />
      </section>
      <section className="mx-auto max-w-6xl px-6 py-12">
        <div className="grid gap-6 md:grid-cols-2">
          <TopProvidersCard c={c} windowKey={windowKey} refreshTick={refreshTick} />
          <TopUsersCard c={c} windowKey={windowKey} refreshTick={refreshTick} />
        </div>
      </section>
      <section className="mx-auto max-w-6xl px-6 pb-24">
        <SelfServiceCard c={c} />
      </section>
    </>
  );
}

type BoardCopy = (typeof copy)["zh"]["board"] | (typeof copy)["en"]["board"];

function BoardHero({ c, windowKey, onWindowChange, onRefresh, refreshing, lastUpdated }: {
  c: BoardCopy;
  windowKey: WindowKey;
  onWindowChange: (next: WindowKey) => void;
  onRefresh: () => void;
  refreshing: boolean;
  lastUpdated: Date | null;
}) {
  const updatedLabel = lastUpdated ? c.lastUpdated(lastUpdated.toLocaleTimeString()) : "";
  return (
    <section className="relative mx-auto max-w-6xl px-6 pb-6 pt-12 md:pt-16">
      <Confetti density="low" />
      <div className="relative z-10 inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-amber-300 px-3 py-1 text-xs font-extrabold uppercase tracking-wider">
        <Sparkles size={14} /> Live
      </div>
      <h1 className="relative z-10 mt-3 font-display text-4xl font-extrabold leading-[1.05] md:text-6xl">{c.title}</h1>
      <p className="relative z-10 mt-3 max-w-2xl text-base text-slate-700 md:text-lg">{c.subtitle}</p>
      <div className="relative z-10 mt-2 inline-flex items-start gap-2 rounded-2xl border-2 border-slate-300 bg-white px-3 py-1.5 text-xs text-slate-600">
        <ShieldAlert size={14} className="mt-0.5 shrink-0 text-amber-600" /> {c.privacyNotice}
      </div>
      <div className="relative z-10 mt-5 flex flex-wrap items-center gap-3">
        <span className="text-xs font-bold uppercase tracking-wider text-slate-500">{c.windowLabel}</span>
        <div role="tablist" aria-label={c.windowLabel} className="inline-flex rounded-full border-2 border-slate-800 bg-white p-0.5">
          {(["24h", "7d", "30d"] as WindowKey[]).map((key) => (
            <button
              key={key}
              role="tab"
              aria-selected={windowKey === key}
              type="button"
              onClick={() => onWindowChange(key)}
              className={`rounded-full px-3 py-1.5 text-sm font-extrabold transition-colors ${windowKey === key ? "bg-violet-500 text-white" : "text-slate-700 hover:bg-amber-100"}`}
            >
              {c.windows[key]}
            </button>
          ))}
        </div>
        <button
          type="button"
          onClick={onRefresh}
          disabled={refreshing}
          aria-label={c.refresh}
          className="inline-flex items-center gap-1.5 rounded-full border-2 border-slate-800 bg-white px-3 py-1.5 text-sm font-bold hover:bg-amber-300 disabled:opacity-60"
        >
          <RefreshCw size={14} className={refreshing ? "motion-safe:animate-spin" : ""} />
          {refreshing ? c.refreshing : c.refresh}
        </button>
        {updatedLabel && <span className="text-xs text-slate-500">{updatedLabel}</span>}
      </div>
    </section>
  );
}

function KpiCard({ label, value, sub, color, icon }: { label: string; value: string; sub?: string; color: string; icon: React.ReactNode }) {
  return (
    <div className="sticker bg-white p-5 lift">
      <div className="flex items-start gap-3">
        <div className={`rounded-full border-2 border-slate-800 p-2 ${color}`}>{icon}</div>
        <div className="min-w-0 flex-1">
          <div className="text-xs font-bold uppercase tracking-wider text-slate-500">{label}</div>
          <div className="mt-1 font-display text-2xl font-extrabold text-slate-900 break-all">{value}</div>
          {sub && <div className="mt-0.5 text-xs text-slate-500">{sub}</div>}
        </div>
      </div>
    </div>
  );
}

function KpiRow({ c, windowKey, refreshTick, onUpdated }: { c: BoardCopy; windowKey: WindowKey; refreshTick: number; onUpdated: (d: Date) => void }) {
  const [data, setData] = useState<KpisResponse | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    apiGet<KpisResponse>(`/v1/public/dashboard/kpis?window=${windowKey}`)
      .then((value) => {
        if (cancelled) return;
        setData(value);
        onUpdated(new Date());
      })
      .catch(() => {
        if (!cancelled) setData(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [windowKey, refreshTick, onUpdated]);

  if (loading && !data) {
    return (
      <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
        {[0, 1, 2, 3].map((i) => (
          <Skeleton key={i} className="h-24 rounded-2xl" />
        ))}
      </div>
    );
  }

  const k = data;
  return (
    <div className="grid gap-4 md:grid-cols-2 lg:grid-cols-4">
      <KpiCard label={c.kpis.totalSpend} value={formatMoneyFull(k?.totalSpendUsd)} sub={`${formatInt(k?.totalRequests)} ${c.kpis.totalRequests.toLowerCase()}`} color="bg-violet-400 text-white" icon={<Coins size={18} />} />
      <KpiCard label={c.kpis.totalTopup} value={formatMoneyFull(k?.totalTopupUsd)} sub={`${formatInt(k?.totalTopupOrders)} orders`} color="bg-amber-300" icon={<Sparkles size={18} />} />
      <KpiCard label={c.kpis.activeApiUsers} value={formatInt(k?.activeApiUsers)} sub={`${formatInt(k?.registeredUsers)} ${c.kpis.registeredUsers}`} color="bg-pink-400 text-white" icon={<Users size={18} />} />
      <KpiCard label={c.kpis.onlineShares} value={formatInt(k?.onlineShares)} sub={`${formatInt(k?.activeProviders)} ${c.kpis.activeProviders}`} color="bg-emerald-400 text-white" icon={<Layers size={18} />} />
    </div>
  );
}

function TrendCard({ c, refreshTick }: { c: BoardCopy; refreshTick: number }) {
  const [data, setData] = useState<TrendResponse | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    apiGet<TrendResponse>(`/v1/public/dashboard/trend?days=${TREND_DAYS}`)
      .then((value) => { if (!cancelled) setData(value); })
      .catch(() => { if (!cancelled) setData(null); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [refreshTick]);

  const series = data?.series ?? [];
  const chartData = series.map((point) => ({
    date: point.date,
    spend: Number(point.spendUsd) || 0,
    topup: Number(point.topupUsd) || 0,
  }));
  const hasData = chartData.some((p) => p.spend > 0 || p.topup > 0);

  return (
    <div className="sticker bg-white p-5 md:p-6">
      <div className="mb-3 flex flex-wrap items-baseline justify-between gap-2">
        <div>
          <h2 className="font-display text-xl font-extrabold">{c.trend.title}</h2>
          <p className="mt-0.5 text-xs text-slate-500">{c.trend.subtitle} · {TREND_DAYS}d</p>
        </div>
      </div>
      {loading && !data ? (
        <Skeleton className="h-72 w-full rounded-2xl" />
      ) : !hasData ? (
        <EmptyState shape="circle" title={c.empty.title} hint={c.empty.body} />
      ) : (
        <MemphisLineChart
          data={chartData}
          xKey="date"
          series={[
            { key: "topup", label: c.trend.seriesTopup, color: "#FBBF24" },
            { key: "spend", label: c.trend.seriesSpend, color: "#8B5CF6" },
          ]}
          ariaLabel={c.trend.title}
          formatYTick={(v) => formatMoney(v)}
          formatTooltipValue={(v) => formatMoney(typeof v === "number" ? v : Number(v))}
          formatXTick={(v) => String(v).slice(5)}
          height={300}
        />
      )}
    </div>
  );
}

function BreakdownGrid({ c, refreshTick }: { c: BoardCopy; refreshTick: number }) {
  return (
    <div className="grid gap-6 lg:grid-cols-3">
      <BreakdownCard title={c.breakdown.appType} dim="app_type" refreshTick={refreshTick} valueAs="requests" />
      <BreakdownCard title={c.breakdown.model} dim="model" refreshTick={refreshTick} valueAs="spend" />
      <BreakdownCard title={c.breakdown.provider} dim="provider" refreshTick={refreshTick} valueAs="spend" />
    </div>
  );
}

function BreakdownCard({ title, dim, refreshTick, valueAs }: { title: string; dim: string; refreshTick: number; valueAs: "spend" | "requests" }) {
  const [data, setData] = useState<BreakdownResponse | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    apiGet<BreakdownResponse>(`/v1/public/dashboard/breakdown?dim=${dim}&days=${BREAKDOWN_DAYS}`)
      .then((value) => { if (!cancelled) setData(value); })
      .catch(() => { if (!cancelled) setData(null); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [dim, refreshTick]);

  const buckets = (data?.buckets ?? []).slice(0, 7);
  const others = (data?.buckets ?? []).slice(7);
  const pieData: Array<{ name: string; value: number }> = buckets.map((b) => ({
    name: b.name || "—",
    value: valueAs === "spend" ? Number(b.spendUsd) || 0 : b.requests,
  }));
  if (others.length > 0) {
    const sum = others.reduce((acc, b) => acc + (valueAs === "spend" ? Number(b.spendUsd) || 0 : b.requests), 0);
    if (sum > 0) pieData.push({ name: "Other", value: sum });
  }
  const hasData = pieData.length > 0 && pieData.some((p) => p.value > 0);
  return (
    <div className="sticker bg-white p-5">
      <h3 className="font-display text-lg font-extrabold">{title}</h3>
      <p className="text-xs text-slate-500">{BREAKDOWN_DAYS}d</p>
      <div className="mt-3">
        {loading && !data ? (
          <Skeleton className="h-60 w-full rounded-2xl" />
        ) : !hasData ? (
          <div className="flex h-60 items-center justify-center text-sm text-slate-400">—</div>
        ) : (
          <MemphisPieChart
            data={pieData}
            ariaLabel={title}
            height={240}
            formatValue={(v) => valueAs === "spend" ? formatMoney(typeof v === "number" ? v : Number(v)) : formatInt(typeof v === "number" ? v : Number(v))}
          />
        )}
      </div>
    </div>
  );
}

function TopModelsCard({ c, windowKey, refreshTick }: { c: BoardCopy; windowKey: WindowKey; refreshTick: number }) {
  const [data, setData] = useState<ModelRow[] | null>(null);
  const [loading, setLoading] = useState(true);
  const days = windowKey === "24h" ? 1 : windowKey === "7d" ? 7 : 30;

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    apiGet<{ items: ModelRow[] }>(`/v1/public/dashboard/top-models?days=${days}&limit=${TOP_LIMIT}`)
      .then((value) => { if (!cancelled) setData(value.items ?? []); })
      .catch(() => { if (!cancelled) setData([]); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [days, refreshTick]);

  const columns: Column<ModelRow>[] = [
    { key: "appType", header: c.topModels.colAppType, render: (r) => <span className="rounded-full border-2 border-slate-300 bg-white px-2 py-0.5 text-xs font-extrabold uppercase">{r.appType}</span> },
    { key: "model", header: c.topModels.colModel, render: (r) => <span className="font-mono text-sm break-all">{r.model}</span> },
    { key: "spendUsd", header: c.topModels.colSpend, render: (r) => <span className="font-mono font-extrabold">{formatMoneyFull(r.spendUsd)}</span> },
    { key: "requests", header: c.topModels.colRequests, render: (r) => <span className="font-mono">{formatInt(r.requests)}</span> },
    { key: "uniqueUsers", header: c.topModels.colUsers, render: (r) => <span className="font-mono">{formatInt(r.uniqueUsers)}</span> },
  ];
  return (
    <div className="sticker bg-white p-5 md:p-6">
      <div className="mb-3 flex flex-wrap items-baseline justify-between gap-2">
        <div>
          <h2 className="font-display text-xl font-extrabold flex items-center gap-2"><Cpu size={18} /> {c.topModels.title}</h2>
          <p className="mt-0.5 text-xs text-slate-500">{c.windows[windowKey]} · top {TOP_LIMIT}</p>
        </div>
      </div>
      <DataTable
        rows={data ?? []}
        loading={loading}
        rowKey={(r, i) => `${r.appType}:${r.model}:${i}`}
        columns={columns}
        empty={<EmptyState shape="square" title={c.empty.title} hint={c.empty.body} />}
      />
    </div>
  );
}

function TopProvidersCard({ c, windowKey, refreshTick }: { c: BoardCopy; windowKey: WindowKey; refreshTick: number }) {
  const [data, setData] = useState<ProviderRow[] | null>(null);
  const [loading, setLoading] = useState(true);
  const days = windowKey === "24h" ? 1 : windowKey === "7d" ? 7 : 30;

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    apiGet<{ items: ProviderRow[] }>(`/v1/public/dashboard/top-providers?days=${days}&limit=${TOP_LIMIT}`)
      .then((value) => { if (!cancelled) setData(value.items ?? []); })
      .catch(() => { if (!cancelled) setData([]); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [days, refreshTick]);

  const columns: Column<ProviderRow>[] = [
    { key: "ownerEmail", header: c.topProviders.colEmail, render: (r) => <span className="font-mono text-xs break-all text-slate-700">{r.ownerEmail}</span> },
    { key: "grossSpendUsd", header: c.topProviders.colSpend, render: (r) => <span className="font-mono font-extrabold">{formatMoneyFull(r.grossSpendUsd)}</span> },
    { key: "requests", header: c.topProviders.colRequests, render: (r) => <span className="font-mono">{formatInt(r.requests)}</span> },
    { key: "uniqueShares", header: c.topProviders.colShares, render: (r) => <span className="font-mono">{formatInt(r.uniqueShares)}</span> },
  ];
  return (
    <div className="sticker bg-white p-5 md:p-6">
      <h2 className="font-display text-xl font-extrabold flex items-center gap-2"><Layers size={18} /> {c.topProviders.title}</h2>
      <p className="mt-0.5 text-xs text-slate-500">{c.topProviders.subtitle} · {c.windows[windowKey]}</p>
      <div className="mt-3">
        <DataTable
          rows={data ?? []}
          loading={loading}
          rowKey={(r, i) => `${r.ownerEmail}:${i}`}
          columns={columns}
          empty={<EmptyState shape="circle" title={c.empty.title} hint={c.empty.body} />}
        />
      </div>
    </div>
  );
}

function TopUsersCard({ c, windowKey, refreshTick }: { c: BoardCopy; windowKey: WindowKey; refreshTick: number }) {
  const [data, setData] = useState<UserRow[] | null>(null);
  const [loading, setLoading] = useState(true);
  const days = windowKey === "24h" ? 1 : windowKey === "7d" ? 7 : 30;

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    apiGet<{ items: UserRow[] }>(`/v1/public/dashboard/top-users?days=${days}&limit=${TOP_LIMIT}`)
      .then((value) => { if (!cancelled) setData(value.items ?? []); })
      .catch(() => { if (!cancelled) setData([]); })
      .finally(() => { if (!cancelled) setLoading(false); });
    return () => { cancelled = true; };
  }, [days, refreshTick]);

  const columns: Column<UserRow>[] = [
    { key: "email", header: c.topUsers.colEmail, render: (r) => <span className="font-mono text-xs break-all text-slate-700">{r.email}</span> },
    { key: "spendUsd", header: c.topUsers.colSpend, render: (r) => <span className="font-mono font-extrabold">{formatMoneyFull(r.spendUsd)}</span> },
    { key: "requests", header: c.topUsers.colRequests, render: (r) => <span className="font-mono">{formatInt(r.requests)}</span> },
  ];
  return (
    <div className="sticker bg-white p-5 md:p-6">
      <h2 className="font-display text-xl font-extrabold flex items-center gap-2"><Users size={18} /> {c.topUsers.title}</h2>
      <p className="mt-0.5 text-xs text-slate-500">{c.topUsers.subtitle} · {c.windows[windowKey]}</p>
      <div className="mt-3">
        <DataTable
          rows={data ?? []}
          loading={loading}
          rowKey={(r, i) => `${r.email}:${i}`}
          columns={columns}
          empty={<EmptyState shape="square" title={c.empty.title} hint={c.empty.body} />}
        />
      </div>
    </div>
  );
}

function SelfServiceCard({ c }: { c: BoardCopy }) {
  return (
    <div className="relative overflow-hidden sticker bg-amber-200 p-8 md:p-10">
      <span aria-hidden className="pointer-events-none absolute -right-12 -top-12 h-44 w-44 rounded-full bg-amber-300/70" />
      <span aria-hidden className="pointer-events-none absolute bottom-6 left-6 h-16 w-16 rotate-12 rounded-3xl border-2 border-slate-800 bg-violet-300" />
      <div className="relative z-10 max-w-2xl">
        <h2 className="font-display text-3xl font-extrabold md:text-4xl">{c.selfService.title}</h2>
        <p className="mt-3 text-base text-slate-800">{c.selfService.body}</p>
        <div className="mt-6 flex flex-wrap gap-3">
          <Link href="/dashboard" className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2.5 font-bold text-white btn-pop">
            {c.selfService.cta} <ArrowRight size={16} />
          </Link>
          <Link href="/claim" className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-5 py-2.5 font-bold text-slate-900 btn-pop">
            {c.selfService.ctaProvider}
          </Link>
        </div>
      </div>
    </div>
  );
}
