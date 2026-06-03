"use client";

import { useEffect, useState, type ReactNode } from "react";
import { useForm } from "react-hook-form";
import {
  Wallet,
  KeyRound,
  Activity,
  LayoutDashboard,
  PlusCircle,
  ArrowRightLeft,
  Copy,
  Trash2,
  Pencil,
  ShieldAlert
} from "lucide-react";
import { useMarketAuth } from "@/components/auth";
import { useToast } from "@/components/ui/Toast";
import { Tabs } from "@/components/ui/Tabs";
import { StatCard } from "@/components/ui/StatCard";
import { Pill } from "@/components/ui/Pill";
import { Modal, ModalActions } from "@/components/ui/Modal";
import { DataTable, type DataTableProps } from "@/components/ui/DataTable";
import { EmptyState } from "@/components/ui/EmptyState";
import { Skeleton } from "@/components/ui/Skeleton";
import { PageHeader } from "@/components/ui/PageHeader";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "@/components/shadcn/form";
import { apiDelete, apiGet, apiGetAllItems, apiPost } from "@/lib/client-api";
import { useLocale } from "@/components/language-provider";
import { copy } from "@/lib/copy";
import { useDateTimeFormatter } from "@/lib/time";
import { formatCommissionRate, usePublicConfig } from "@/lib/public-config";

type WalletSummary = { user_cash_usd: string; user_reserved_usd: string };
type ClaimSummary = { available_usd: string };
type ApiKeyItem = {
  id: string;
  name: string;
  prefix: string;
  usage_tokens?: number | string | null;
  usage_amount?: string | number | null;
  scope_json?: unknown;
  expires_at?: string | null;
  monthly_spend_cap?: string | null;
  last_used_at?: string | null;
  last_used_ip_country?: string | null;
  created_at: string;
  revoked_at?: string | null;
  paused_at?: string | null;
  deleted_at?: string | null;
};

type ApiKeySecretItem = {
  api_key_id: string;
  prefix: string;
  key: string;
  created_at: string;
};
type PriceItem = {
  id: string;
  model_id?: string | null;
  app_type: string;
  model_pattern: string;
  display_name?: string | null;
  status?: string;
};
type AvailableShareItem = {
  router_id: string;
  share_id: string;
  owner_email?: string | null;
  subdomain?: string | null;
  app_type: string;
  capabilities: string[];
  online: boolean;
  for_sale: string;
  share_status: string;
};
type ApiKeyShareRef = {
  router_id: string;
  share_id: string;
};
type ApiKeyLimitFormValue = {
  expiresMode: "unlimited" | "custom";
  expiresAt: string;
  spendMode: "unlimited" | "custom";
  monthlySpendCap: string;
  selectedVendorKeys: string[];
  schedulingProfile: string;
};
type MoneyEvent = {
  id?: string;
  event_id?: string;
  event_type?: string;
  reference_type?: string;
  reference_id?: string;
  amount?: string;
  gross_amount?: string;
  fee_amount?: string;
  net_amount?: string;
  status?: string;
  from_account_type?: string;
  to_account_type?: string;
  created_at?: string;
};
type UsageItem = {
  id?: string;
  request_id?: string;
  api_key_name?: string | null;
  api_key_prefix?: string | null;
  model?: string;
  app_type?: string;
  request_agent?: string;
  requested_model?: string;
  actual_model?: string;
  actual_model_source?: string;
  share_subdomain?: string | null;
  status?: string;
  input_tokens?: number;
  output_tokens?: number;
  cache_read_tokens?: number;
  cache_write_tokens?: number;
  usage_amount?: string;
  reserved_amount?: string;
  gross_amount?: string;
  fee_amount?: string;
  net_amount?: string;
  price_snapshot?: unknown;
  created_at?: string;
};
type TicketItem = {
  id: string;
  ticket_no?: string;
};

const USER_TABLE_PAGE_SIZE_KEY = "cc-switch-market:user-table-page-size";
const USER_TABLE_PAGE_SIZE_OPTIONS = [10, 20, 50, 100];
const AGENT_MODEL_VENDOR_OPTIONS = [
  {
    agent: "claude",
    label: "Claude",
    vendors: [
      { id: "anthropic", label: "Anthropic" },
      { id: "openai", label: "OpenAI" },
      { id: "gemini", label: "Gemini" },
      { id: "deepseek", label: "DeepSeek" },
    ],
  },
  {
    agent: "codex",
    label: "Codex",
    vendors: [
      { id: "openai", label: "OpenAI" },
      { id: "anthropic", label: "Anthropic" },
      { id: "gemini", label: "Gemini" },
      { id: "deepseek", label: "DeepSeek" },
    ],
  },
  {
    agent: "gemini",
    label: "Gemini",
    vendors: [
      { id: "gemini", label: "Gemini" },
      { id: "openai", label: "OpenAI" },
      { id: "anthropic", label: "Anthropic" },
      { id: "deepseek", label: "DeepSeek" },
    ],
  },
] as const;
const DEFAULT_AGENT_VENDOR_KEYS = ["claude:anthropic", "codex:openai", "gemini:gemini"];
const SCHEDULING_PROFILE_OPTIONS = [
  "balanced",
  "price-first",
  "stability-first",
  "fresh-quota",
  "diversify",
  "premium",
  "budget-aware",
] as const;
const DEFAULT_SCHEDULING_PROFILE = "balanced";

function ConsoleDataTable<T>(props: DataTableProps<T>) {
  const { locale } = useLocale();
  return (
    <DataTable
      {...props}
      pagination
      pageSize={20}
      pageSizeOptions={USER_TABLE_PAGE_SIZE_OPTIONS}
      pageSizeStorageKey={USER_TABLE_PAGE_SIZE_KEY}
      paginationLabels={{
        previous: locale === "zh" ? "上一页" : "Prev",
        next: locale === "zh" ? "下一页" : "Next",
        rowsPerPage: locale === "zh" ? "每页" : "Rows",
      }}
    />
  );
}

export function DashboardRoot() {
  const { user } = useMarketAuth();
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard;
  const publicConfig = usePublicConfig();

  const TABS = [
    { key: "overview", label: c.tabs.overview, icon: <LayoutDashboard size={16} /> },
    { key: "wallet", label: c.tabs.wallet, icon: <Wallet size={16} /> },
    { key: "keys", label: c.tabs.keys, icon: <KeyRound size={16} /> },
    { key: "usage", label: c.tabs.usage, icon: <Activity size={16} /> }
  ];

  // 检测 admin 拒绝跳转
  useEffect(() => {
    if (typeof window === "undefined") return;
    const params = new URLSearchParams(window.location.search);
    if (params.get("denied") === "admin") {
      toast.push({ variant: "warning", title: c.adminDeniedTitle, description: c.adminDeniedDesc });
      params.delete("denied");
      const search = params.toString();
      const url = `${window.location.pathname}${search ? `?${search}` : ""}${window.location.hash}`;
      window.history.replaceState(null, "", url);
    }
  }, [toast, c]);

  return (
    <div className="grid gap-6">
      <PageHeader
        title={c.title}
        subtitle={user ? <span>{c.subtitleAuthed(user.email)}</span> : c.subtitleAnon}
      />
      <Tabs items={TABS} defaultKey="overview" storageKey="cc-switch-market:dashboard-tab">
        {(active) => {
          if (active === "overview") return <Overview />;
          if (active === "wallet") return <WalletTab />;
          if (active === "keys") return <KeysTab />;
          if (active === "usage") return <UsageTab />;
          return null;
        }}
      </Tabs>
    </div>
  );
}

function Overview() {
  const { locale } = useLocale();
  const c = copy[locale].dashboard;
  const publicConfig = usePublicConfig();
  const [wallet, setWallet] = useState<WalletSummary | null>(null);
  const [keys, setKeys] = useState<ApiKeyItem[] | null>(null);
  const [events, setEvents] = useState<MoneyEvent[] | null>(null);
  useEffect(() => {
    apiGet<WalletSummary>("/v1/wallet/summary").then(setWallet).catch(() => setWallet({ user_cash_usd: "0", user_reserved_usd: "0" }));
    apiGet<ApiKeyItem[]>("/v1/api-keys").then(setKeys).catch(() => setKeys([]));
    apiGetAllItems<MoneyEvent>("/v1/money-events").then(setEvents).catch(() => setEvents([]));
  }, []);

  const activeKeys = (keys ?? []).filter((k) => !k.revoked_at && !k.paused_at && !k.deleted_at).length;
  return (
    <div className="grid gap-6">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard label={c.overview.statBalance} value={wallet ? formatWalletUsd(wallet.user_cash_usd) : <Skeleton />} color="violet" icon={<Wallet size={16} />} loading={!wallet} />
        <StatCard label={c.overview.statLocked} value={wallet ? formatWalletUsd(wallet.user_reserved_usd) : <Skeleton />} color="amber" icon={<ShieldAlert size={16} />} loading={!wallet} />
        <StatCard label={c.overview.statKeys} value={keys ? activeKeys : <Skeleton />} color="emerald" icon={<KeyRound size={16} />} loading={!keys} />
        <StatCard label={c.overview.statRecent} value={events ? events.length : <Skeleton />} color="pink" icon={<Activity size={16} />} loading={!events} />
      </div>
      <div className="sticker bg-white p-6">
        <h2 className="font-display text-2xl font-extrabold">{c.overview.recentTitle}</h2>
        <div className="mt-4">
          {!events && (
            <div className="grid gap-2">
              <Skeleton className="h-12 w-full rounded-2xl" />
              <Skeleton className="h-12 w-full rounded-2xl" />
            </div>
          )}
          {events && events.length === 0 && (
            <EmptyState shape="circle" title={c.overview.emptyTitle} hint={c.overview.emptyHint} />
          )}
          {events && events.length > 0 && (
            <div className="grid gap-2">
              {events.slice(0, 6).map((e, i) => (
                <div key={String(e.id ?? i)} className="flex flex-wrap items-center justify-between gap-3 rounded-2xl border-2 border-slate-800 bg-amber-50/40 p-3">
                  <div>
                    <div className="font-bold">{labelEventType(e.event_type ?? e.reference_type, c.events, publicConfig)}</div>
                    <div className="text-xs text-slate-500">{e.from_account_type} → {e.to_account_type}</div>
                  </div>
                  <div className="font-mono text-sm">${e.net_amount ?? e.amount ?? "0"}</div>
                </div>
              ))}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}

function formatWalletUsd(value: string | null | undefined): string {
  const amount = Number(value ?? 0);
  if (!Number.isFinite(amount)) return "$0.00";
  return `$${amount.toFixed(2)}`;
}

function WalletTab() {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard;
  const publicConfig = usePublicConfig();
  const [summary, setSummary] = useState<WalletSummary | null>(null);
  const [claimSummary, setClaimSummary] = useState<ClaimSummary | null>(null);
  const [items, setItems] = useState<MoneyEvent[] | null>(null);
  const [topupOpen, setTopupOpen] = useState(false);
  const [transferOpen, setTransferOpen] = useState(false);
  const formatDate = useDateTimeFormatter();
  const claimAvailable = claimSummary?.available_usd ?? "0";
  const canTransferClaim = Number(claimAvailable) > 0;

  function reload() {
    apiGet<WalletSummary>("/v1/wallet/summary").then(setSummary).catch(() => setSummary({ user_cash_usd: "0", user_reserved_usd: "0" }));
    apiGet<ClaimSummary>("/v1/provider/claim/summary").then(setClaimSummary).catch(() => setClaimSummary({ available_usd: "0" }));
    apiGetAllItems<MoneyEvent>("/v1/wallet/ledger").then(setItems).catch(() => setItems([]));
  }
  useEffect(() => { reload(); }, []);
  useEffect(() => {
    if (typeof window === "undefined") return;
    const params = new URLSearchParams(window.location.search);
    const topupId = params.get("topup_id");
    if (!topupId) return;
    let stopped = false;
    let attempts = 0;
    async function poll() {
      attempts += 1;
      try {
        const order = await apiGet<{ status?: string; net_amount?: string }>(`/v1/topups/${topupId}`);
        if (stopped) return;
        if (order.status && order.status !== "pending") {
          toast.push({
            variant: order.status === "paid" ? "success" : "info",
            title: order.status === "paid" ? c.wallet.topupPaidTitle : c.wallet.topupStatusTitle(order.status),
            description: order.status === "paid" ? c.wallet.topupNetDesc(order.net_amount ?? "0") : undefined
          });
          params.delete("topup_id");
          params.delete("mock_topup");
          const search = params.toString();
          window.history.replaceState(null, "", `${window.location.pathname}${search ? `?${search}` : ""}${window.location.hash}`);
          reload();
          return;
        }
      } catch {
        // Keep polling briefly; webhook arrival can lag behind the payment redirect.
      }
      if (!stopped && attempts < 20) {
        window.setTimeout(poll, 3000);
      }
    }
    poll();
    return () => { stopped = true; };
  }, [toast]);

  return (
    <div className="grid gap-6">
      <div className="sticker bg-white p-6">
        <div className="flex flex-wrap items-end justify-between gap-4">
          <div>
            <div className="text-xs font-bold uppercase tracking-wider text-slate-500">{c.wallet.balance}</div>
            <div className="mt-1 max-w-full break-words font-display text-4xl font-extrabold leading-tight sm:text-5xl">{formatWalletUsd(summary?.user_cash_usd)}</div>
            <div className="mt-1 max-w-full break-words text-sm text-slate-500">{c.wallet.lockedPrefix} {formatWalletUsd(summary?.user_reserved_usd)}</div>
          </div>
          <div className="flex flex-wrap gap-2">
            {canTransferClaim && (
              <button className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-emerald-300 px-5 py-3 font-bold text-slate-900 lift" onClick={() => setTransferOpen(true)}>
                <ArrowRightLeft size={16} /> {c.wallet.transferClaim}
              </button>
            )}
            <button className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-3 font-bold text-white btn-pop" onClick={() => setTopupOpen(true)}>
              <PlusCircle size={16} /> {c.wallet.topup}
            </button>
          </div>
        </div>
      </div>
      <ConsoleDataTable
        rows={items ?? []}
        loading={items === null}
        rowKey={(row, idx) => String(row.id ?? idx)}
        empty={<EmptyState shape="circle" title={c.wallet.emptyTitle} hint={c.wallet.emptyHint} />}
        columns={[
          { key: "type", header: c.wallet.colType, mobileLabel: c.wallet.colType, render: (r) => <span className="font-bold">{labelEventType(r.event_type ?? r.reference_type, c.events, publicConfig)}</span> },
          { key: "amount", header: c.wallet.colAmount, mobileLabel: c.wallet.colAmount, render: (r) => <span className="font-mono">${r.net_amount ?? r.amount ?? "0"}</span> },
          { key: "from", header: c.wallet.colAccount, mobileLabel: c.wallet.colAccount, render: (r) => <span className="text-xs text-slate-500">{r.from_account_type} → {r.to_account_type}</span> },
          { key: "ref", header: c.wallet.colRef, mobileLabel: c.wallet.colRef, render: (r) => <span className="font-mono text-xs text-slate-500 break-all">{r.reference_id}</span> },
          { key: "time", header: c.wallet.colTime, mobileLabel: c.wallet.colTime, render: (r) => <span className="text-xs text-slate-500">{formatDate(r.created_at)}</span> }
        ]}
      />
      <TopupModal open={topupOpen} onClose={() => { setTopupOpen(false); reload(); }} />
      <TransferClaimModal open={transferOpen} onClose={() => { setTransferOpen(false); reload(); }} max={claimAvailable} />
    </div>
  );
}

function TopupModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.wallet;
  const [submitting, setSubmitting] = useState(false);
  const form = useForm<{ amount: string }>({ defaultValues: { amount: "10" } });

  useEffect(() => {
    if (open) form.reset({ amount: "10" });
  }, [open, form]);

  async function submit(values: { amount: string }) {
    const amount = values.amount;
    if (!amount || Number(amount) <= 0) {
      form.setError("amount", { type: "validate", message: c.topupErrorInvalidDesc });
      return;
    }
    if (Number(amount) > 1000) {
      form.setError("amount", { type: "validate", message: c.topupFirstLimitDesc });
      return;
    }
    setSubmitting(true);
    try {
      const order = await apiPost<{ checkout_url?: string }>("/v1/topups/checkout", { amount_usd: amount });
      if (order.checkout_url) {
        window.location.href = order.checkout_url;
        return;
      }
      toast.push({ variant: "info", title: c.topupOrderCreated, description: c.topupOrderNoUrl });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.topupErrorTitle, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={c.topupModalTitle}
      description={c.topupModalDesc}
      footer={
        <>
          <button type="button" onClick={onClose} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.topupCancel}</button>
          <button type="submit" form="topup-form" disabled={submitting} className="rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">
            {submitting ? c.topupSubmitting : c.topupSubmit}
          </button>
        </>
      }
    >
      <Form {...form}>
        <form id="topup-form" onSubmit={form.handleSubmit(submit)} className="grid gap-3">
          <FormField
            control={form.control}
            name="amount"
            rules={{ required: c.topupErrorInvalidDesc }}
            render={({ field }) => (
              <FormItem>
                <FormLabel className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.topupAmountLabel}</FormLabel>
                <FormControl>
                  <input
                    type="number"
                    min="1"
                    step="0.01"
                    {...field}
                    className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 text-2xl font-bold outline-none focus:bg-white"
                  />
                </FormControl>
                <FormMessage className="text-xs font-bold text-pink-600" />
              </FormItem>
            )}
          />
          <div className="mt-1 flex flex-wrap gap-2">
            {["5", "10", "25", "50", "100", "500", "1000"].map((v) => (
              <button key={v} type="button" onClick={() => form.setValue("amount", v, { shouldDirty: true, shouldValidate: true })} className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-sm font-bold lift">${v}</button>
            ))}
          </div>
        </form>
      </Form>
    </Modal>
  );
}

function TransferClaimModal({ open, onClose, max }: { open: boolean; onClose: () => void; max: string }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.wallet;
  const [submitting, setSubmitting] = useState(false);
  const form = useForm<{ amount: string }>({ defaultValues: { amount: max } });

  useEffect(() => {
    if (open) form.reset({ amount: max });
  }, [open, max, form]);

  async function submit(values: { amount: string }) {
    const amount = values.amount;
    const value = Number(amount);
    const maxValue = Number(max);
    if (!Number.isFinite(value) || !Number.isFinite(maxValue) || value <= 0 || value > maxValue) {
      form.setError("amount", { type: "validate", message: c.transferClaimInvalidDesc });
      return;
    }
    setSubmitting(true);
    try {
      await apiPost("/v1/provider/claim/convert-to-balance", { amount_usd: amount });
      toast.push({ variant: "success", title: c.transferClaimSuccess });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.transferClaimErrorTitle, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={c.transferClaimModalTitle}
      description={c.transferClaimModalDesc}
      footer={
        <>
          <button type="button" onClick={onClose} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.topupCancel}</button>
          <button type="submit" form="transfer-claim-form" disabled={submitting} className="rounded-full border-2 border-slate-800 bg-emerald-300 px-5 py-2 font-bold text-slate-900 btn-pop disabled:opacity-50">
            {submitting ? c.transferClaimSubmitting : c.transferClaimSubmit}
          </button>
        </>
      }
    >
      <Form {...form}>
        <form id="transfer-claim-form" onSubmit={form.handleSubmit(submit)} className="grid gap-3">
          <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 p-3 text-sm font-bold">
            {c.transferClaimAvailable(max)}
          </div>
          <FormField
            control={form.control}
            name="amount"
            rules={{ required: c.transferClaimInvalidDesc }}
            render={({ field }) => (
              <FormItem>
                <FormLabel className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.transferClaimAmountLabel}</FormLabel>
                <FormControl>
                  <input
                    type="number"
                    min="0.01"
                    step="0.01"
                    {...field}
                    className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 text-2xl font-bold outline-none focus:bg-white"
                  />
                </FormControl>
                <FormMessage className="text-xs font-bold text-pink-600" />
              </FormItem>
            )}
          />
        </form>
      </Form>
    </Modal>
  );
}

function KeysTab() {
  const { user, showLogin } = useMarketAuth();
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  const [items, setItems] = useState<ApiKeyItem[] | null>(null);
  const [secrets, setSecrets] = useState<ApiKeySecretItem[] | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<ApiKeyItem | null>(null);
  const [pauseTarget, setPauseTarget] = useState<ApiKeyItem | null>(null);
  const [activateTarget, setActivateTarget] = useState<ApiKeyItem | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<ApiKeyItem | null>(null);
  const [shareTarget, setShareTarget] = useState<ApiKeyItem | null>(null);
  const formatDate = useDateTimeFormatter();

  function reload() {
    if (!user) {
      setItems([]);
      setSecrets([]);
      return;
    }
    apiGet<ApiKeyItem[]>("/v1/api-keys").then(setItems).catch(() => setItems([]));
    apiGet<{ items: ApiKeySecretItem[] }>("/v1/api-key-secrets").then((value) => setSecrets(value.items)).catch(() => setSecrets([]));
  }
  useEffect(() => { reload(); }, [user?.id]);

  async function copyValue(value: string) {
    try {
      await navigator.clipboard.writeText(value);
      toast.push({ variant: "success", title: c.copied });
    } catch {
      toast.push({ variant: "error", title: c.copyFailed });
    }
  }

  if (!user) {
    return (
      <div className="grid gap-6">
        <div>
          <h2 className="font-display text-2xl font-extrabold">{c.title}</h2>
          <p className="text-sm text-slate-500">{c.subtitle}</p>
        </div>
        <div className="sticker bg-amber-50 p-8 text-center">
          <div className="font-display text-2xl font-extrabold">{c.anonTitle}</div>
          <p className="mt-2 text-slate-600">{c.anonHint}</p>
          <button onClick={showLogin} className="mt-4 inline-flex rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">{c.openLogin}</button>
        </div>
      </div>
    );
  }

  return (
    <div className="grid gap-6">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="font-display text-2xl font-extrabold">{c.title}</h2>
          <p className="text-sm text-slate-500">{c.subtitle}</p>
        </div>
        <button onClick={() => setCreateOpen(true)} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop">
          <PlusCircle size={16} /> {c.createBtn}
        </button>
      </div>

      <ConsoleDataTable
        rows={(items ?? []).filter((item) => !item.deleted_at)}
        loading={items === null}
        rowKey={(row) => row.id}
        empty={<EmptyState shape="square" title={c.emptyTitle} hint={c.emptyHint} action={<button onClick={() => setCreateOpen(true)} className="rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop">{c.emptyAction}</button>} />}
        columns={[
          { key: "name", header: c.colName, render: (r) => <span className="font-bold">{r.name}</span> },
          { key: "key", header: c.colKey, render: (r) => <ApiKeySecretCell item={r} secret={secrets?.find((secret) => secret.api_key_id === r.id)} onCopy={copyValue} /> },
          { key: "status", header: c.colStatus, render: (r) => <ApiKeyUsageStatus item={r} /> },
          { key: "limits", header: c.colLimits, render: (r) => <ApiKeyLimits item={r} /> },
          { key: "last", header: c.colLast, render: (r) => <span className="text-xs text-slate-500">{r.last_used_at ? formatDate(r.last_used_at) : c.lastNever}</span> },
          { key: "actions", header: c.colActions, render: (r) => (
            <div className="flex flex-wrap gap-2">
              {!r.paused_at && <button onClick={() => setEditTarget(r)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift"><Pencil size={12} /> {c.actionEdit}</button>}
              {!r.paused_at && <button onClick={() => setShareTarget(r)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-amber-100 px-3 py-1 text-xs font-bold lift"><ShieldAlert size={12} /> {c.actionShares}</button>}
              {!r.paused_at && <button onClick={() => setPauseTarget(r)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-pink-200 px-3 py-1 text-xs font-bold lift"><Trash2 size={12} /> {c.actionPause}</button>}
              {r.paused_at && <button onClick={() => setActivateTarget(r)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-emerald-200 px-3 py-1 text-xs font-bold lift"><ShieldAlert size={12} /> {c.actionActivate}</button>}
              {r.paused_at && <button onClick={() => setDeleteTarget(r)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-pink-300 px-3 py-1 text-xs font-bold lift"><Trash2 size={12} /> {c.actionDelete}</button>}
            </div>
          ) }
        ]}
      />

      <CreateKeyModal open={createOpen} onClose={() => setCreateOpen(false)} onCreated={reload} />
      <EditKeyModal target={editTarget} onClose={() => setEditTarget(null)} onDone={reload} />
      <PauseKeyModal target={pauseTarget} onClose={() => setPauseTarget(null)} onDone={reload} />
      <ActivateKeyModal target={activateTarget} onClose={() => setActivateTarget(null)} onDone={reload} />
      <DeleteKeyModal target={deleteTarget} onClose={() => setDeleteTarget(null)} onDone={reload} />
      <ShareAllowlistModal target={shareTarget} onClose={() => setShareTarget(null)} onDone={reload} />
    </div>
  );
}

function CreateKeyModal({ open, onClose, onCreated }: { open: boolean; onClose: () => void; onCreated: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  const [name, setName] = useState("");
  const [prices, setPrices] = useState<PriceItem[] | null>(null);
  const [limits, setLimits] = useState<ApiKeyLimitFormValue>(emptyLimitForm());
  const [submitting, setSubmitting] = useState(false);
  useEffect(() => {
    if (!open) return;
    apiGet<PriceItem[]>("/v1/prices").then(setPrices).catch(() => setPrices([]));
  }, [open]);

  function resetForm() {
    setName("");
    setLimits(emptyLimitForm());
  }

  function close() {
    resetForm();
    onClose();
  }

  async function submit() {
    setSubmitting(true);
    try {
      const payload: Record<string, unknown> = { name: name || c.defaultName, ...buildLimitPayload(limits) };
      await apiPost<{ item: ApiKeySecretItem }>("/v1/api-key-secrets", payload);
      onCreated();
      close();
      toast.push({ variant: "success", title: c.createSuccess });
    } catch (err) {
      toast.push({ variant: "error", title: c.createFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }
  const createDisabled = submitting
    || !isLimitFormSubmittable(limits);
  return (
    <Modal open={open} onClose={close} title={c.createTitle} description={c.createDesc}>
      <label className="grid gap-2">
        <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.createNameLabel}</span>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder={c.createNamePlaceholder} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      </label>
      <ApiKeyLimitFields value={limits} onChange={setLimits} prices={prices} />
      <ModalActions>
        <button onClick={close} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.createCancel}</button>
        <button onClick={submit} disabled={createDisabled} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">{submitting ? c.createSubmitting : c.createSubmit}</button>
      </ModalActions>
    </Modal>
  );
}

function EditKeyModal({ target, onClose, onDone }: { target: ApiKeyItem | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  const [name, setName] = useState("");
  const [prices, setPrices] = useState<PriceItem[] | null>(null);
  const [limits, setLimits] = useState<ApiKeyLimitFormValue>(emptyLimitForm());
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!target) return;
    setName(target.name);
    setLimits(limitFormFromApiKey(target));
    apiGet<PriceItem[]>("/v1/prices").then(setPrices).catch(() => setPrices([]));
  }, [target?.id]);

  async function submit() {
    if (!target) return;
    setSubmitting(true);
    try {
      await apiPost<ApiKeyItem>(`/v1/api-keys/${target.id}`, { name: name || c.defaultName });
      await apiPost<ApiKeyItem>(`/v1/api-keys/${target.id}/limits`, buildLimitPayload(limits));
      toast.push({ variant: "success", title: c.editSaved });
      onDone();
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.editFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={!!target} onClose={onClose} title={c.editTitle} description={target ? c.editDesc(target.name, target.prefix) : ""}>
      <label className="grid gap-2">
        <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.createNameLabel}</span>
        <input value={name} onChange={(e) => setName(e.target.value)} placeholder={c.createNamePlaceholder} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      </label>
      <ApiKeyLimitFields value={limits} onChange={setLimits} prices={prices} />
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.createCancel}</button>
        <button onClick={submit} disabled={submitting || !isLimitFormSubmittable(limits)} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">{submitting ? c.editSaving : c.editSave}</button>
      </ModalActions>
    </Modal>
  );
}

function ApiKeyLimitFields({ value, onChange, prices }: { value: ApiKeyLimitFormValue; onChange: (value: ApiKeyLimitFormValue) => void; prices: PriceItem[] | null }) {
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;

  function patch(next: Partial<ApiKeyLimitFormValue>) {
    onChange({ ...value, ...next });
  }

  function toggleVendorKey(key: string) {
    patch({
      selectedVendorKeys: value.selectedVendorKeys.includes(key)
        ? value.selectedVendorKeys.filter((item) => item !== key)
        : [...value.selectedVendorKeys, key],
    });
  }

  return (
    <div className="mt-4 grid gap-4">
      <div className="rounded-3xl border-2 border-slate-800 bg-white p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="font-bold">{c.expiresLabel}</div>
            <div className="text-xs text-slate-500">{c.unlimitedDefault}</div>
          </div>
          <select value={value.expiresMode} onChange={(e) => patch({ expiresMode: e.target.value as "unlimited" | "custom" })} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-bold">
            <option value="unlimited">{c.unlimited}</option>
            <option value="custom">{c.custom}</option>
          </select>
        </div>
        {value.expiresMode === "custom" && (
          <input type="datetime-local" value={value.expiresAt} onChange={(e) => patch({ expiresAt: e.target.value })} className="mt-3 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
        )}
      </div>
      <div className="rounded-3xl border-2 border-slate-800 bg-white p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="font-bold">{c.spendCapLabel}</div>
            <div className="text-xs text-slate-500">{c.spendCapHint}</div>
          </div>
          <select value={value.spendMode} onChange={(e) => patch({ spendMode: e.target.value as "unlimited" | "custom" })} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-bold">
            <option value="unlimited">{c.unlimited}</option>
            <option value="custom">{c.custom}</option>
          </select>
        </div>
        {value.spendMode === "custom" && (
          <input type="number" min="0" step="0.01" value={value.monthlySpendCap} onChange={(e) => patch({ monthlySpendCap: e.target.value })} placeholder="100" className="mt-3 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 font-mono outline-none focus:bg-white" />
        )}
      </div>
      <div className="rounded-3xl border-2 border-slate-800 bg-white p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="font-bold">{c.schedulingLabel}</div>
            <div className="text-xs text-slate-500">{c.schedulingHint}</div>
          </div>
          <select value={value.schedulingProfile} onChange={(e) => patch({ schedulingProfile: e.target.value })} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-bold">
            {SCHEDULING_PROFILE_OPTIONS.map((profile) => (
              <option key={profile} value={profile}>{c.schedulingProfiles[profile] ?? profile}</option>
            ))}
          </select>
        </div>
      </div>
      <div className="rounded-3xl border-2 border-slate-800 bg-white p-4">
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <div className="font-bold">{c.modelScopeLabel}</div>
            <div className="text-xs text-slate-500">{c.modelScopeHint}</div>
          </div>
        </div>
        <div className="mt-3 grid gap-3 rounded-2xl border-2 border-slate-800 bg-amber-50 p-3">
          {prices === null && <div className="text-sm text-slate-500">{c.modelsLoading}</div>}
          {prices && AGENT_MODEL_VENDOR_OPTIONS.map((agent) => (
            <div key={agent.agent} className="rounded-xl border-2 border-slate-200 bg-white p-3">
              <div className="mb-2 text-sm font-black">{agent.label}</div>
              <div className="flex flex-wrap gap-2">
                {agent.vendors.map((vendor) => {
                  const key = `${agent.agent}:${vendor.id}`;
                  const checked = value.selectedVendorKeys.includes(key);
                  return (
                    <label key={key} className="inline-flex cursor-pointer items-center gap-2 rounded-full border-2 border-slate-800 bg-amber-50 px-3 py-1.5 text-xs font-bold">
                      <input type="checkbox" checked={checked} onChange={() => toggleVendorKey(key)} />
                      <span>{vendor.label}</span>
                    </label>
                  );
                })}
              </div>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

function emptyLimitForm(): ApiKeyLimitFormValue {
  return {
    expiresMode: "unlimited",
    expiresAt: "",
    spendMode: "unlimited",
    monthlySpendCap: "",
    selectedVendorKeys: DEFAULT_AGENT_VENDOR_KEYS,
    schedulingProfile: DEFAULT_SCHEDULING_PROFILE,
  };
}

function limitFormFromApiKey(item: ApiKeyItem): ApiKeyLimitFormValue {
  const vendorKeys = apiKeyVendorKeys(item.scope_json);
  return {
    expiresMode: item.expires_at ? "custom" : "unlimited",
    expiresAt: item.expires_at ? toDateTimeLocalValue(item.expires_at) : "",
    spendMode: item.monthly_spend_cap ? "custom" : "unlimited",
    monthlySpendCap: item.monthly_spend_cap ? trimMoney(item.monthly_spend_cap) : "",
    selectedVendorKeys: vendorKeys ?? DEFAULT_AGENT_VENDOR_KEYS,
    schedulingProfile: apiKeySchedulingProfile(item.scope_json) ?? DEFAULT_SCHEDULING_PROFILE,
  };
}

function buildLimitPayload(value: ApiKeyLimitFormValue): Record<string, unknown> {
  return {
    expires_at: value.expiresMode === "custom" ? new Date(value.expiresAt).toISOString() : null,
    monthly_spend_cap: value.spendMode === "custom" ? value.monthlySpendCap : null,
    scope_json: {
      agent_model_vendors: agentModelVendorsFromKeys(value.selectedVendorKeys),
      schedulingProfile: value.schedulingProfile,
    },
  };
}

function isLimitFormSubmittable(value: ApiKeyLimitFormValue): boolean {
  return !(value.expiresMode === "custom" && !value.expiresAt)
    && !(value.spendMode === "custom" && value.monthlySpendCap === "")
    && value.selectedVendorKeys.length > 0;
}

function apiKeyModelIds(scope: unknown): string[] | null {
  if (!scope || typeof scope !== "object") return null;
  const models = (scope as { model_ids?: unknown }).model_ids;
  if (!Array.isArray(models)) return null;
  return models.filter((value): value is string => typeof value === "string");
}

function apiKeyModelKeys(scope: unknown): string[] | null {
  if (!scope || typeof scope !== "object") return null;
  const access = (scope as { model_access?: unknown; modelAccess?: unknown }).model_access ?? (scope as { modelAccess?: unknown }).modelAccess;
  if (!access || typeof access !== "object") return null;
  const keys: string[] = [];
  for (const [app, slots] of Object.entries(access as Record<string, unknown>)) {
    if (!slots || typeof slots !== "object") continue;
    for (const [slot, models] of Object.entries(slots as Record<string, unknown>)) {
      if (!Array.isArray(models)) continue;
      for (const model of models) {
        if (typeof model === "string") keys.push(`${app}:${slot}:${model}`);
      }
    }
  }
  return keys.length ? keys : null;
}

function apiKeySchedulingProfile(scope: unknown): string | null {
  if (!scope || typeof scope !== "object") return null;
  const raw = (scope as { schedulingProfile?: unknown; scheduling_profile?: unknown }).schedulingProfile
    ?? (scope as { scheduling_profile?: unknown }).scheduling_profile;
  if (typeof raw !== "string") return null;
  const value = raw.toLowerCase().replace(/_/g, "-");
  return (SCHEDULING_PROFILE_OPTIONS as readonly string[]).includes(value) ? value : null;
}

function apiKeyVendorKeys(scope: unknown): string[] | null {
  if (!scope || typeof scope !== "object") return null;
  const access = (scope as { agent_model_vendors?: unknown; agentModelVendors?: unknown }).agent_model_vendors
    ?? (scope as { agentModelVendors?: unknown }).agentModelVendors;
  if (!access || typeof access !== "object") return null;
  const keys: string[] = [];
  for (const [agent, vendors] of Object.entries(access as Record<string, unknown>)) {
    if (!Array.isArray(vendors)) continue;
    for (const vendor of vendors) {
      if (typeof vendor === "string") keys.push(`${agent}:${vendor}`);
    }
  }
  return keys.length ? keys : null;
}

function agentModelVendorsFromKeys(keys: string[]): Record<string, string[]> {
  const access: Record<string, string[]> = {};
  for (const key of keys) {
    const [agent, vendor] = key.split(":");
    if (!agent || !vendor) continue;
    access[agent] ??= [];
    if (!access[agent].includes(vendor)) access[agent].push(vendor);
  }
  return access;
}

function toDateTimeLocalValue(value: string): string {
  const date = new Date(value);
  if (Number.isNaN(date.getTime())) return "";
  const offsetMs = date.getTimezoneOffset() * 60 * 1000;
  return new Date(date.getTime() - offsetMs).toISOString().slice(0, 16);
}

function ApiKeySecretCell({ item, secret, onCopy }: { item: ApiKeyItem; secret?: ApiKeySecretItem; onCopy: (value: string) => void }) {
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  return (
    <div className="flex items-center gap-2">
      <span className="font-mono text-sm">{item.prefix}***</span>
      <button
        onClick={() => secret && onCopy(secret.key)}
        disabled={!secret}
        title={secret ? c.copyTitle : c.copyDisabledTitle}
        className="rounded-full border-2 border-slate-800 bg-white p-1.5 lift disabled:cursor-not-allowed disabled:opacity-40"
        aria-label={c.copyAria}
      >
        <Copy size={14} />
      </button>
    </div>
  );
}

function ApiKeyUsageStatus({ item }: { item: ApiKeyItem }) {
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  return (
    <div className="grid gap-1">
      <div className="text-xs font-bold leading-tight text-slate-600">
        {c.usageLine(formatCompactNumber(item.usage_tokens), formatCompactMoney(item.usage_amount))}
      </div>
      <div>
        <Pill variant={item.paused_at ? "warning" : "success"}>{item.paused_at ? c.statusPaused : c.statusActive}</Pill>
      </div>
    </div>
  );
}

function ApiKeyLimits({ item }: { item: ApiKeyItem }) {
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  const modelCount = apiKeyModelCount(item.scope_json);
  const profile = apiKeySchedulingProfile(item.scope_json) ?? DEFAULT_SCHEDULING_PROFILE;
  const formatDate = useDateTimeFormatter();
  return (
    <div className="grid gap-1 text-xs text-slate-600">
      <span>{c.limitExpires}: {item.expires_at ? formatDate(item.expires_at) : c.unlimited}</span>
      <span>{c.limitSpend}: {item.monthly_spend_cap ? `$${trimMoney(item.monthly_spend_cap)}` : c.unlimited}</span>
      <span>{c.limitModels}: {modelCount === null ? c.unlimited : c.modelCount(modelCount)}</span>
      <span>{c.limitScheduling}: {c.schedulingProfiles[profile] ?? profile}</span>
    </div>
  );
}

function apiKeyModelCount(scope: unknown): number | null {
  return apiKeyVendorKeys(scope)?.length ?? apiKeyModelKeys(scope)?.length ?? apiKeyModelIds(scope)?.length ?? 3;
}

function trimMoney(value: string): string {
  if (!value.includes(".")) return value;
  return value.replace(/(\.\d*?[1-9])0+$/, "$1").replace(/\.0+$/, "");
}

function formatCompactNumber(value: unknown): string {
  const numeric = typeof value === "number" ? value : Number(value ?? 0);
  if (!Number.isFinite(numeric)) return "0";
  return new Intl.NumberFormat("en-US", {
    notation: "compact",
    maximumFractionDigits: 1,
    minimumFractionDigits: 1,
  }).format(numeric);
}

function formatCompactMoney(value: unknown): string {
  const numeric = typeof value === "number" ? value : Number(value ?? 0);
  if (!Number.isFinite(numeric)) return "$0";
  return `$${formatCompactNumber(numeric)}`;
}

function PauseKeyModal({ target, onClose, onDone }: { target: ApiKeyItem | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  async function submit() {
    if (!target) return;
    try {
      await apiPost(`/v1/api-keys/${target.id}/status`, { action: "pause" });
      toast.push({ variant: "success", title: c.paused });
      onDone();
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.operationFailed, description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title={c.pauseTitle} description={target ? c.pauseDesc(target.name, target.prefix) : ""}>
      <ModalActions>
        <button onClick={onClose} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.createCancel}</button>
        <button onClick={submit} className="rounded-full border-2 border-slate-800 bg-pink-400 px-5 py-2 font-bold text-white btn-pop">{c.pauseConfirm}</button>
      </ModalActions>
    </Modal>
  );
}

function ActivateKeyModal({ target, onClose, onDone }: { target: ApiKeyItem | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  async function submit() {
    if (!target) return;
    try {
      await apiPost(`/v1/api-keys/${target.id}/status`, { action: "activate" });
      toast.push({ variant: "success", title: c.activated });
      onDone();
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.operationFailed, description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title={c.activateTitle} description={target ? c.activateDesc(target.name, target.prefix) : ""}>
      <ModalActions>
        <button onClick={onClose} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.createCancel}</button>
        <button onClick={submit} className="rounded-full border-2 border-slate-800 bg-emerald-400 px-5 py-2 font-bold text-white btn-pop">{c.activateConfirm}</button>
      </ModalActions>
    </Modal>
  );
}

function DeleteKeyModal({ target, onClose, onDone }: { target: ApiKeyItem | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  async function submit() {
    if (!target) return;
    try {
      await apiDelete(`/v1/api-keys/${target.id}`, { confirm: true });
      toast.push({ variant: "success", title: c.deleted });
      onDone();
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.operationFailed, description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title={c.deleteTitle} description={c.deleteDesc}>
      <ModalActions>
        <button onClick={onClose} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.createCancel}</button>
        <button onClick={submit} className="rounded-full border-2 border-slate-800 bg-pink-400 px-5 py-2 font-bold text-white btn-pop">{c.deleteConfirm}</button>
      </ModalActions>
    </Modal>
  );
}

function ShareAllowlistModal({ target, onClose, onDone }: { target: ApiKeyItem | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].dashboard.keys;
  const [available, setAvailable] = useState<AvailableShareItem[] | null>(null);
  const [selected, setSelected] = useState<string[]>([]);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => {
    if (!target) return;
    setAvailable(null);
    setSelected([]);
    apiGet<{ items: AvailableShareItem[] }>("/v1/me/available-shares")
      .then((value) => setAvailable(value.items))
      .catch(() => setAvailable([]));
    apiGet<{ shares: ApiKeyShareRef[] }>(`/v1/api-keys/${target.id}/share-allowlist`)
      .then((value) => setSelected(value.shares.map(shareKey)))
      .catch(() => setSelected([]));
  }, [target?.id]);

  function toggle(item: AvailableShareItem) {
    const key = shareKey(item);
    setSelected((current) => current.includes(key) ? current.filter((value) => value !== key) : [...current, key]);
  }

  async function save(nextSelected = selected) {
    if (!target) return;
    setSubmitting(true);
    try {
      const shares = nextSelected.map(parseShareKey).filter((value): value is ApiKeyShareRef => value !== null);
      await apiPost(`/v1/api-keys/${target.id}/share-allowlist`, { shares });
      toast.push({ variant: "success", title: c.shareSaved });
      onDone();
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.operationFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  const rows = available ?? [];
  const ownerGroups = Array.from(new Set(rows.map((item) => item.owner_email || "").filter(Boolean))).sort();
  function selectOwner(ownerEmail: string) {
    const ownerKeys = rows
      .filter((item) => item.owner_email === ownerEmail)
      .map(shareKey);
    setSelected((current) => Array.from(new Set([...current, ...ownerKeys])));
  }
  function removeOwner(ownerEmail: string) {
    const ownerKeys = new Set(rows
      .filter((item) => item.owner_email === ownerEmail)
      .map(shareKey));
    setSelected((current) => current.filter((key) => !ownerKeys.has(key)));
  }
  function replaceWithOwner(ownerEmail: string) {
    setSelected(rows.filter((item) => item.owner_email === ownerEmail).map(shareKey));
  }
  return (
    <Modal open={!!target} onClose={onClose} title={c.shareTitle} description={target ? c.shareDesc(target.name, target.prefix) : ""} width="lg">
      <div className="grid gap-4">
        <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 p-3 text-sm font-bold">
          {selected.length === 0 ? c.shareAutoMode : c.shareLimitedMode(selected.length)}
        </div>
        {ownerGroups.length > 0 && (
          <div className="rounded-2xl border-2 border-slate-800 bg-white p-3">
            <div className="mb-2 text-xs font-black uppercase tracking-wider text-slate-600">{c.shareOwnerQuickSelect}</div>
            <div className="flex flex-wrap gap-2">
              {ownerGroups.map((owner) => {
                const ownerKeys = rows.filter((item) => item.owner_email === owner).map(shareKey);
                const ownerSelected = ownerKeys.length > 0 && ownerKeys.every((key) => selected.includes(key));
                return (
                  <span key={owner} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-amber-50 px-2 py-1 text-xs">
                    <span className="font-mono font-bold">{owner}</span>
                    <button
                      type="button"
                      onClick={() => ownerSelected ? removeOwner(owner) : selectOwner(owner)}
                      className={`rounded-full px-2 py-0.5 font-bold text-white ${ownerSelected ? "bg-pink-500" : "bg-violet-500"}`}
                    >
                      {ownerSelected ? c.shareOwnerRemove : c.shareOwnerAdd}
                    </button>
                    <button type="button" onClick={() => replaceWithOwner(owner)} className="rounded-full bg-slate-900 px-2 py-0.5 font-bold text-white">{c.shareOwnerOnly}</button>
                  </span>
                );
              })}
            </div>
          </div>
        )}
        <div className="max-h-96 overflow-auto rounded-2xl border-2 border-slate-800 bg-white p-3">
          {available === null && <div className="text-sm text-slate-500">{c.sharesLoading}</div>}
          {available && rows.length === 0 && <div className="text-sm text-slate-500">{c.sharesEmpty}</div>}
          {rows.map((item) => {
            const key = shareKey(item);
            const checked = selected.includes(key);
            return (
              <label key={key} className="mb-2 flex cursor-pointer items-start gap-3 rounded-xl border-2 border-slate-200 bg-amber-50 px-3 py-2 text-sm last:mb-0">
                <input type="checkbox" checked={checked} onChange={() => toggle(item)} className="mt-1" />
                <div className="min-w-0 flex-1">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="rounded-full border border-slate-800 bg-white px-2 py-0.5 font-mono text-xs font-bold">{item.subdomain || item.share_id}</span>
                    {!item.online && <span className="rounded-full bg-pink-100 px-2 py-0.5 text-xs font-bold">{c.shareOffline}</span>}
                  </div>
                  <div className="mt-1 break-all font-mono text-xs text-slate-500">{item.router_id} / {item.share_id}</div>
                  <div className="mt-1 break-all font-mono text-xs font-bold text-slate-700">{c.shareOwner}: {item.owner_email || "-"}</div>
                  <div className="mt-2 flex flex-wrap gap-1">
                    {item.capabilities.map((capability) => (
                      <span key={capability} className="rounded-full bg-violet-100 px-2 py-0.5 text-[11px] font-bold uppercase">{capability}</span>
                    ))}
                  </div>
                </div>
              </label>
            );
          })}
        </div>
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.createCancel}</button>
        <button onClick={() => save([])} disabled={submitting} className="mt-5 rounded-full border-2 border-slate-800 bg-amber-100 px-5 py-2 font-bold lift disabled:opacity-50">{c.shareClear}</button>
        <button onClick={() => save()} disabled={submitting} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">{submitting ? c.editSaving : c.editSave}</button>
      </ModalActions>
    </Modal>
  );
}

function shareKey(item: ApiKeyShareRef): string {
  return `${item.router_id}\u0000${item.share_id}`;
}

function parseShareKey(value: string): ApiKeyShareRef | null {
  const [router_id, share_id] = value.split("\u0000");
  if (!router_id || !share_id) return null;
  return { router_id, share_id };
}

function UsageTab() {
  const { locale } = useLocale();
  const c = copy[locale].dashboard.usage;
  const toast = useToast();
  const [items, setItems] = useState<UsageItem[] | null>(null);
  const [reportingId, setReportingId] = useState<string | null>(null);
  const [reportTarget, setReportTarget] = useState<UsageItem | null>(null);
  const formatDate = useDateTimeFormatter();

  useEffect(() => {
    apiGetAllItems<UsageItem>("/v1/usage").then(setItems).catch(() => setItems([]));
  }, []);

  async function reportUsage(row: UsageItem) {
    if (!row.id || row.status !== "settled") return;
    setReportingId(row.id);
    try {
      const ticket = await apiPost<TicketItem>(`/v1/usage/${row.id}/report`, {});
      toast.push({ variant: "success", title: c.reportCreated, description: ticket.ticket_no });
      window.location.href = `/support?ticket=${encodeURIComponent(ticket.id)}`;
    } catch (err) {
      toast.push({ variant: "error", title: c.reportFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setReportingId(null);
    }
  }

  return (
    <div className="grid gap-4">
      <ConsoleDataTable
        rows={items ?? []}
        loading={items === null}
        rowKey={(row, i) => String(row.id ?? row.request_id ?? i)}
        empty={<EmptyState shape="triangle" title={c.emptyTitle} hint={c.emptyHint} />}
        rowClassName={(r) => r.status === "needs_review" ? "bg-pink-50" : ""}
        columns={[
          { key: "rid", header: c.colRequest, render: (r) => <span className="font-mono text-xs break-all">{r.request_id}</span> },
          { key: "apiKey", header: c.colApiKey, render: (r) => <span className="font-mono text-xs font-bold text-slate-700">{r.api_key_name || "-"}</span> },
          { key: "model", header: c.colModel, render: (r) => <span><span className="rounded-full bg-violet-100 border border-slate-800 px-2 py-0.5 text-xs font-bold uppercase mr-1">{r.request_agent ?? r.app_type}</span><span className="font-mono text-sm">{r.actual_model ?? r.model}</span><span className="ml-1 text-xs text-slate-500">({r.requested_model ?? r.model})</span></span> },
          { key: "share", header: c.colShare, render: (r) => r.share_subdomain ? <span className="rounded-full border border-slate-800 bg-amber-100 px-2 py-0.5 font-mono text-xs font-bold">{r.share_subdomain}</span> : <span /> },
          { key: "tokens", header: c.colTokens, render: (r) => <span className="text-xs text-slate-600">in {r.input_tokens ?? 0} · out {r.output_tokens ?? 0}</span> },
          { key: "amount", header: c.colAmount, render: (r) => <span className="font-mono">${r.gross_amount ?? r.usage_amount ?? r.reserved_amount ?? "0"}</span> },
          { key: "status", header: c.colStatus, render: (r) => <Pill status={r.status ?? "pending"} /> },
          { key: "time", header: c.colTime, render: (r) => <span className="text-xs text-slate-500">{formatDate(r.created_at)}</span> },
          {
            key: "actions",
            header: c.colActions,
            render: (r) => {
              const canReport = r.status === "settled" && !!r.id;
              return (
                <button
                  type="button"
                  onClick={() => setReportTarget(r)}
                  disabled={!canReport || reportingId === r.id}
                  className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift disabled:opacity-40"
                >
                  {c.report}
                </button>
              );
            }
          }
        ]}
        expandable={(r) => (
          <div className="grid gap-2 text-sm">
            <div className="font-bold">{c.snapshotTitle}</div>
            <pre className="overflow-auto rounded-xl border-2 border-slate-800 bg-slate-900 p-3 text-xs text-emerald-200">{JSON.stringify(r.price_snapshot ?? {}, null, 2)}</pre>
            <div className="text-xs text-slate-500">{c.cacheReadPrefix} {r.cache_read_tokens ?? 0} · {c.cacheWritePrefix} {r.cache_write_tokens ?? 0}</div>
          </div>
        )}
      />
      <Modal
        open={!!reportTarget}
        onClose={() => setReportTarget(null)}
        title={c.reportConfirmTitle}
        description={c.reportConfirmBody}
        footer={
          <ModalActions>
            <button
              type="button"
              onClick={() => setReportTarget(null)}
              className="rounded-full border-2 border-slate-800 bg-white px-4 py-2 text-sm font-bold lift"
            >
              {c.reportCancel}
            </button>
            <button
              type="button"
              onClick={() => {
                const row = reportTarget;
                setReportTarget(null);
                if (row) void reportUsage(row);
              }}
              disabled={!!reportingId}
              className="rounded-full border-2 border-slate-800 bg-pink-500 px-4 py-2 text-sm font-bold text-white lift disabled:opacity-50"
            >
              {c.reportConfirm}
            </button>
          </ModalActions>
        }
      >
        <div className="grid gap-2 rounded-2xl border-2 border-slate-800 bg-amber-50 p-4 text-sm">
          <div className="font-mono text-xs break-all">{reportTarget?.request_id ?? "-"}</div>
          <div className="text-slate-600">{c.colAmount}: ${reportTarget?.gross_amount ?? reportTarget?.usage_amount ?? reportTarget?.reserved_amount ?? "0"}</div>
        </div>
      </Modal>
    </div>
  );
}

type EventDict = Record<string, string>;
function labelEventType(t: string | undefined, dict: EventDict, publicConfig?: { marketCommissionBps?: number; routerCommissionBps?: number }): ReactNode {
  switch (t) {
    case "topup": return dict.topup;
    case "topup_fee": return dict.topupFee;
    case "request_charge":
    case "usage_charge": return dict.usage;
    case "usage_reserved": return dict.usageReserved;
    case "usage_release":
    case "reservation_refund": return dict.usageRelease;
    case "platform_commission": return dict.marketCommissionWithRate?.replace("{rate}", formatCommissionRate(publicConfig?.marketCommissionBps)) ?? dict.platformCommission;
    case "router_commission": return dict.routerCommissionWithRate?.replace("{rate}", formatCommissionRate(publicConfig?.routerCommissionBps)) ?? dict.routerCommission;
    case "ledger_entry": return dict.ledgerEntry;
    case "provider_income": return dict.providerIncome;
    case "provider_earning_to_balance": return dict.providerEarningToBalance;
    case "provider_earning_transfer": return dict.providerEarningTransfer;
    case "payout": return dict.payout;
    case "payout_fee": return dict.payoutFee;
    case "payout_reserved": return dict.payoutReserved;
    case "payout_released": return dict.payoutReleased;
    case "refund": return dict.refund;
    case "manual_adjustment":
    case "adjustment": return dict.adjustment;
    default: return t || dict.fallback;
  }
}
