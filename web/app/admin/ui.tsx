"use client";

import { useEffect, useState, type ReactNode } from "react";
import { useForm } from "react-hook-form";
import {
  LayoutDashboard,
  Users,
  CreditCard,
  ArrowLeft,
  Tags,
  Share2,
  Receipt,
  Coins,
  Banknote,
  LifeBuoy,
  BookOpen,
  Activity,
  ClipboardList,
  RotateCw,
  ShieldCheck,
  AlertTriangle,
  RefreshCw,
  Bell,
  Pencil,
  Plus,
  Trash2,
  Copy,
  Settings
} from "lucide-react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Tabs, type TabItem } from "@/components/ui/Tabs";
import { StatCard } from "@/components/ui/StatCard";
import { Pill } from "@/components/ui/Pill";
import { Modal, ModalActions } from "@/components/ui/Modal";
import { DataTable, type Column, type DataTableProps } from "@/components/ui/DataTable";
import { EmptyState } from "@/components/ui/EmptyState";
import { Skeleton } from "@/components/ui/Skeleton";
import { MoneyAmount } from "@/components/ui/MoneyAmount";
import { useToast } from "@/components/ui/Toast";
import { Switch } from "@/components/shadcn/switch";
import { Tooltip, TooltipContent, TooltipTrigger } from "@/components/shadcn/tooltip";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage } from "@/components/shadcn/form";
import { apiDelete, apiGet, apiGetAllItems, apiPatchJson, apiPost, apiPutBytes, apiPutJson } from "@/lib/client-api";
import { readAnnouncements, type SiteAnnouncement, writeAnnouncements } from "@/lib/site-announcements";
import { useLocale } from "@/components/language-provider";
import { copy } from "@/lib/copy";
import { formatCommissionRate, updateCachedPublicConfig, usePublicConfig } from "@/lib/public-config";
import { formatDateTime, formatUtcOffset, useDateTimeFormatter } from "@/lib/time";
import { FilePicker, ImageAttachmentGrid } from "@/app/claim/ui";

type AnyRow = Record<string, unknown>;

function AdminDataTable<T>(props: DataTableProps<T>) {
  const { locale } = useLocale();
  const publicConfig = usePublicConfig();
  return (
    <DataTable
      {...props}
      pagination
      pageSize={publicConfig.adminTablePageSize}
      paginationLabels={{ previous: locale === "zh" ? "上一页" : "Prev", next: locale === "zh" ? "下一页" : "Next" }}
    />
  );
}

export function AdminRoot() {
  const { locale } = useLocale();
  const c = copy[locale].admin;
  const TABS: TabItem[] = [
    { key: "overview", label: c.tabs.overview, icon: <LayoutDashboard size={16} /> },
    { key: "users", label: c.tabs.users, icon: <Users size={16} /> },
    { key: "models", label: c.tabs.models, icon: <Tags size={16} /> },
    { key: "shares", label: c.tabs.shares, icon: <Share2 size={16} /> },
    { key: "money", label: c.tabs.money, icon: <Banknote size={16} /> },
    { key: "tickets", label: c.tabs.tickets, icon: <LifeBuoy size={16} /> },
    { key: "announcements", label: c.tabs.announcements, icon: <Bell size={16} /> },
    { key: "settings", label: c.tabs.settings, icon: <Settings size={16} /> },
    { key: "audit", label: c.tabs.audit, icon: <ClipboardList size={16} /> }
  ];
  return (
    <div className="grid gap-6">
      <PageHeader title={c.title} subtitle={c.subtitle} badge={<LedgerLight />} />
      <Tabs items={TABS} defaultKey="overview" storageKey="cc-switch-market:admin-tab">
        {(active) => {
          if (active === "overview") return <Overview />;
          if (active === "users") return <UsersTab />;
          if (active === "models") return <ModelsTab />;
          if (active === "shares") return <SharesTab />;
          if (active === "money") return <MoneyWorkspaceTab />;
          if (active === "tickets") return <TicketsTab />;
          if (active === "announcements") return <AnnouncementsTab />;
          if (active === "settings") return <SettingsTab />;
          if (active === "audit") return <AuditTab />;
          return null;
        }}
      </Tabs>
    </div>
  );
}

function LedgerLight() {
  const { locale } = useLocale();
  const c = copy[locale].admin;
  const [check, setCheck] = useState<{ ok?: boolean } | null>(null);
  useEffect(() => {
    apiGet<{ ok?: boolean }>("/v1/admin/ledger/check").then(setCheck).catch(() => setCheck({ ok: false }));
  }, []);
  if (!check) return <span className="rounded-full border-2 border-slate-800 bg-slate-100 px-3 py-1 text-xs font-bold">{c.ledgerChecking}</span>;
  return check.ok
    ? <span className="rounded-full border-2 border-slate-800 bg-emerald-300 px-3 py-1 text-xs font-bold inline-flex items-center gap-1"><ShieldCheck size={14} /> {c.ledgerOk}</span>
    : <span className="rounded-full border-2 border-slate-800 bg-pink-300 px-3 py-1 text-xs font-bold inline-flex items-center gap-1"><AlertTriangle size={14} /> {c.ledgerBad}</span>;
}

function Overview() {
  const { locale } = useLocale();
  const c = copy[locale].admin;
  const [tickets, setTickets] = useState<AnyRow[] | null>(null);
  const [overview, setOverview] = useState<AnyRow | null>(null);

  useEffect(() => {
    apiGetAllItems<AnyRow>("/v1/admin/tickets").then(setTickets).catch(() => setTickets([]));
    apiGet<AnyRow>("/v1/admin/money/overview").then(setOverview).catch(() => setOverview({ ledgerOk: false }));
  }, []);

  const openTickets = (tickets ?? []).filter((t) => ["open", "waiting_admin"].includes(String(t.status))).length;

  return (
    <div className="grid gap-6">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-3">
        <StatCard label={c.overview.todayTopups} value={formatAdminUsd(overview?.todayTopupsUsd)} color="violet" icon={<CreditCard size={16} />} loading={!overview} />
        <StatCard label={c.overview.todayUsage} value={formatAdminUsd(overview?.todayUsageUsd)} color="pink" icon={<Receipt size={16} />} loading={!overview} />
        <StatCard label={c.overview.needsReview} value={Number(overview?.needsReviewCharges ?? 0)} color="amber" icon={<AlertTriangle size={16} />} loading={!overview} />
        <StatCard label={c.overview.pendingPayouts} value={Number(overview?.pendingPayouts ?? 0)} color="emerald" icon={<Banknote size={16} />} loading={!overview} />
        <StatCard label={c.overview.openTickets} value={openTickets} color="violet" icon={<LifeBuoy size={16} />} loading={!tickets} />
        <StatCard label={c.overview.ledger} value={overview?.ledgerOk ? c.overview.ledgerOk : overview ? c.overview.ledgerBad : <Skeleton />} color={overview?.ledgerOk ? "emerald" : "pink"} icon={<ShieldCheck size={16} />} loading={!overview} />
      </div>
    </div>
  );
}

function UsersTab() {
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const [adjustTarget, setAdjustTarget] = useState<AnyRow | null>(null);
  const toast = useToast();
  const formatDate = useDateTimeFormatter();

  function reload() { apiGetAllItems<AnyRow>("/v1/admin/users").then(setItems).catch(() => setItems([])); }
  useEffect(() => { reload(); }, []);

  return (
    <div className="grid gap-4">
      <AdminDataTable
        rows={items ?? []}
        loading={items === null}
        rowKey={(r, i) => String(r.id ?? i)}
        empty={<EmptyState shape="circle" title="还没有用户" />}
        columns={[
          { key: "email", header: "邮箱", render: (r) => <span className="font-mono text-sm break-all">{String(r.email ?? "")}</span> },
          { key: "status", header: "状态", render: (r) => <Pill status={String(r.status ?? "active")} /> },
          { key: "balance", header: "可用 / 锁定", render: (r) => <MoneyAmount gross={String(r.user_cash_usd ?? "0")} fee={String(r.user_reserved_usd ?? "0")} layout="inline" /> },
          { key: "created", header: "注册时间", render: (r) => <span className="text-xs text-slate-500">{formatDate(String(r.created_at ?? ""))}</span> },
          { key: "actions", header: "操作", render: (r) => (
            <button onClick={() => setAdjustTarget(r)} className="rounded-full border-2 border-slate-800 bg-amber-100 px-3 py-1 text-xs font-bold lift">调账</button>
          ) }
        ]}
      />
      <UserAdjustModal target={adjustTarget} onClose={() => setAdjustTarget(null)} onDone={() => { reload(); toast.push({ variant: "success", title: "已调账" }); }} />
    </div>
  );
}

function UserAdjustModal({ target, onClose, onDone }: { target: AnyRow | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const form = useForm<{ amount: string; reason: string }>({
    defaultValues: { amount: "0", reason: "" },
  });
  const [direction, setDirection] = useState<"credit" | "debit">("credit");
  useEffect(() => {
    if (target) {
      setDirection("credit");
      form.reset({ amount: "0", reason: "" });
    }
  }, [target, form]);
  async function submit(values: { amount: string; reason: string }) {
    if (!target) return;
    if (!values.reason.trim()) {
      form.setError("reason", { type: "required", message: "请填写原因" });
      return;
    }
    try {
      await apiPost(`/v1/admin/users/${target.id}/adjust`, { direction, amount_usd: values.amount, reason: values.reason.trim() });
      onDone();
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "调账失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title="人工调账" description={String(target?.email ?? "")}>
      <Form {...form}>
        <form onSubmit={form.handleSubmit(submit)} className="grid gap-4">
          <div className="flex gap-2">
            <button type="button" onClick={() => setDirection("credit")} className={`rounded-full border-2 border-slate-800 px-4 py-2 text-sm font-bold ${direction === "credit" ? "bg-emerald-300" : "bg-white"}`}>+ 增加余额</button>
            <button type="button" onClick={() => setDirection("debit")} className={`rounded-full border-2 border-slate-800 px-4 py-2 text-sm font-bold ${direction === "debit" ? "bg-pink-300" : "bg-white"}`}>− 扣除余额</button>
          </div>
          <FormField
            control={form.control}
            name="amount"
            rules={{ required: "请填写金额" }}
            render={({ field }) => (
              <FormItem>
                <FormLabel className="text-xs font-bold uppercase tracking-wider text-slate-600">金额（USD）</FormLabel>
                <FormControl>
                  <input
                    type="number"
                    step="0.01"
                    min="0"
                    {...field}
                    className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white"
                  />
                </FormControl>
                <FormMessage className="text-xs font-bold text-pink-600" />
              </FormItem>
            )}
          />
          <FormField
            control={form.control}
            name="reason"
            rules={{ required: "请填写原因" }}
            render={({ field }) => (
              <FormItem>
                <FormLabel className="text-xs font-bold uppercase tracking-wider text-slate-600">原因</FormLabel>
                <FormControl>
                  <textarea
                    {...field}
                    placeholder="原因（必填，记入 admin_audit）"
                    className="min-h-24 rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white"
                  />
                </FormControl>
                <FormMessage className="text-xs font-bold text-pink-600" />
              </FormItem>
            )}
          />
          <ModalActions>
            <button type="button" onClick={onClose} className="mt-2 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
            <button type="submit" className="mt-2 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">确认调账</button>
          </ModalActions>
        </form>
      </Form>
    </Modal>
  );
}

function TopupsTab() {
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const [refundTarget, setRefundTarget] = useState<AnyRow | null>(null);
  const [detailTarget, setDetailTarget] = useState<AnyRow | null>(null);
  const formatDate = useDateTimeFormatter();
  function reload() { apiGetAllItems<AnyRow>("/v1/admin/topups").then(setItems).catch(() => setItems([])); }
  useEffect(() => { reload(); }, []);
  return (
    <div className="grid gap-4">
      <AdminDataTable
        rows={items ?? []}
        loading={items === null}
        rowKey={(r, i) => String(r.id ?? i)}
        empty={<EmptyState shape="square" title="还没有充值订单" />}
        columns={[
          { key: "id", header: "订单", render: (r) => <span className="font-mono text-xs break-all">{String(r.id ?? "")}</span> },
          { key: "user", header: "充值用户", render: (r) => <span className="font-mono text-xs break-all">{String(r.user_email ?? "")}</span> },
          { key: "channel", header: "充值渠道", render: (r) => <span className="text-sm font-bold">{paymentMethodLabel(r.payment_method_type)}</span> },
          { key: "amount", header: "金额", render: (r) => <MoneyAmount gross={String(r.gross_amount ?? "0")} fee={String(r.fee_amount ?? "0")} net={String(r.net_amount ?? "0")} layout="inline" /> },
          { key: "status", header: "状态", render: (r) => (
            <div className="flex flex-wrap items-center gap-2">
              <Pill status={String(r.status ?? "pending")} />
              {String(r.status ?? "pending") !== "paid" && (
                <button onClick={() => setDetailTarget(r)} className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift">详情</button>
              )}
            </div>
          ) },
          { key: "time", header: "时间", render: (r) => <span className="text-xs text-slate-500">{formatDate(String(r.created_at ?? ""))}</span> },
          { key: "actions", header: "操作", render: (r) => (
            <div className="flex flex-wrap gap-2">
              <button onClick={() => setDetailTarget(r)} className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift">详情</button>
              {String(r.status) === "paid" && <button onClick={() => setRefundTarget(r)} className="rounded-full border-2 border-slate-800 bg-pink-200 px-3 py-1 text-xs font-bold lift">退款</button>}
            </div>
          ) }
        ]}
      />
      <TopupDetailModal target={detailTarget} onClose={() => setDetailTarget(null)} />
      <RefundTopupModal target={refundTarget} onClose={() => setRefundTarget(null)} onDone={reload} />
    </div>
  );
}

function paymentMethodLabel(value: unknown) {
  const key = String(value ?? "").trim().toLowerCase();
  if (!key) return "—";
  if (["credit", "credit_card", "card"].includes(key)) return "信用卡";
  if (["debit", "debit_card"].includes(key)) return "借记卡";
  if (["wechat", "wechat_pay", "we_chat_pay"].includes(key)) return "微信";
  if (["apple_pay"].includes(key)) return "Apple Pay";
  if (["google_pay"].includes(key)) return "Google Pay";
  if (["crypto", "crypto_currency", "stablecoin", "stablecoins"].includes(key)) return "加密货币";
  return key;
}

function TopupDetailModal({ target, onClose }: { target: AnyRow | null; onClose: () => void }) {
  const [detail, setDetail] = useState<AnyRow | null>(null);
  const [loading, setLoading] = useState(false);
  const formatDate = useDateTimeFormatter();
  useEffect(() => {
    if (!target?.id) {
      setDetail(null);
      return;
    }
    setLoading(true);
    apiGet<AnyRow>(`/v1/admin/topups/${String(target.id)}`)
      .then(setDetail)
      .catch(() => setDetail(null))
      .finally(() => setLoading(false));
  }, [target?.id]);
  const topup = (detail?.topup as AnyRow | undefined) ?? target ?? {};
  return (
    <Modal open={!!target} onClose={onClose} title="充值详情" description={target ? `订单 ${String(target.id ?? "")}` : ""} width="lg">
      {loading ? (
        <div className="grid gap-3">
          <Skeleton className="h-16 rounded-2xl" />
          <Skeleton className="h-40 rounded-2xl" />
        </div>
      ) : (
        <div className="grid gap-4">
          <div className="grid gap-3 md:grid-cols-3">
            <ReviewInfo label="用户" value={<span className="font-mono break-all">{String(topup.user_email ?? "")}</span>} />
            <ReviewInfo label="状态" value={<Pill status={String(topup.status ?? "pending")} />} />
            <ReviewInfo label="渠道" value={paymentMethodLabel(topup.payment_method_type)} />
            <ReviewInfo label="金额" value={`$${String(topup.gross_amount ?? "0")}`} />
            <ReviewInfo label="到账" value={`$${String(topup.net_amount ?? "0")}`} />
            <ReviewInfo label="创建时间" value={formatDate(String(topup.created_at ?? ""))} />
          </div>
          <ReviewSection title="订单上下文">
            <JsonBlock value={topup} />
          </ReviewSection>
          <ReviewSection title="账本记录">
            <JsonBlock value={detail?.ledger ?? []} />
          </ReviewSection>
          <ReviewSection title="Webhook 记录">
            <JsonBlock value={detail?.webhooks ?? []} />
          </ReviewSection>
          <ReviewSection title="对象引用">
            <JsonBlock value={detail?.objects ?? []} />
          </ReviewSection>
          <ReviewSection title="Webhook Payload">
            <JsonBlock value={detail?.webhookPayloads ?? []} />
          </ReviewSection>
        </div>
      )}
    </Modal>
  );
}

function RefundTopupModal({ target, onClose, onDone }: { target: AnyRow | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [reason, setReason] = useState("");
  const [refundFee, setRefundFee] = useState(false);
  useEffect(() => { if (target) { setReason(""); setRefundFee(false); } }, [target]);
  async function submit() {
    if (!target) return;
    if (!reason.trim()) { toast.push({ variant: "error", title: "请填写退款原因" }); return; }
    try {
      await apiPost(`/v1/admin/topups/${target.id}/refund`, { reason: reason.trim(), refund_fee: refundFee });
      toast.push({ variant: "success", title: "已记录退款" });
      onDone(); onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title="充值退款" description={target ? `订单 ${String(target.id)}` : ""}>
      {target && <MoneyAmount gross={String(target.gross_amount ?? "0")} fee={String(target.fee_amount ?? "0")} net={String(target.net_amount ?? "0")} layout="stacked" />}
      <textarea value={reason} onChange={(e) => setReason(e.target.value)} placeholder="退款原因" className="mt-4 min-h-24 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      <label className="mt-3 inline-flex items-center gap-2 text-sm font-bold">
        <input type="checkbox" checked={refundFee} onChange={(e) => setRefundFee(e.target.checked)} className="h-4 w-4" /> 同时退还充值手续费
      </label>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-pink-400 px-5 py-2 font-bold text-white btn-pop">确认退款</button>
      </ModalActions>
    </Modal>
  );
}

function ModelsTab() {
  const toast = useToast();
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const [editorOpen, setEditorOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<AnyRow | null>(null);
  const [appTab, setAppTab] = useState(() => {
    if (typeof window === "undefined") return "all";
    return window.localStorage.getItem("cc-switch-market:admin-models-vendor") || "all";
  });
  const [discountDraft, setDiscountDraft] = useState("10");
  function reload() { apiGet<AnyRow[]>("/v1/admin/models").then(setItems).catch(() => setItems([])); }
  useEffect(() => { reload(); }, []);
  const appTypes = Array.from(new Set((items ?? []).map((item) => String(item.app_type ?? "")).filter(Boolean))).sort();
  useEffect(() => {
    if (!items) return;
    if (appTab !== "all" && !appTypes.includes(appTab)) setAppTab("all");
  }, [appTab, appTypes, items]);
  useEffect(() => {
    if (typeof window !== "undefined") window.localStorage.setItem("cc-switch-market:admin-models-vendor", appTab);
  }, [appTab]);
  const activeVendorItem = appTab === "all" ? undefined : (items ?? []).find((item) => String(item.app_type ?? "") === appTab);
  const activeVendorDiscount = String(((activeVendorItem?.price as AnyRow | undefined)?.discount_percent) ?? "10");
  useEffect(() => {
    setDiscountDraft(activeVendorDiscount);
  }, [activeVendorDiscount]);
  const rows = (appTab === "all" ? (items ?? []) : (items ?? []).filter((item) => String(item.app_type ?? "") === appTab)).slice().sort(compareModelRows);
  async function toggleStatus(row: AnyRow) {
    const id = String(row.id);
    const active = String(row.status ?? "active") === "active";
    try {
      await apiPost(`/v1/admin/models/${id}/${active ? "deactivate" : "activate"}`, {});
      toast.push({ variant: "success", title: active ? "模型已下线" : "模型已上线" });
      reload();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  async function deleteModel(row: AnyRow) {
    if (!window.confirm(`确认删除模型 ${String(row.model_pattern ?? "")}？此操作只允许删除已下线模型`)) return;
    try {
      await apiDelete(`/v1/admin/models/${String(row.id)}`);
      toast.push({ variant: "success", title: "模型已删除" });
      reload();
    } catch (err) {
      toast.push({ variant: "error", title: "删除失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  async function saveDiscount() {
    if (appTab === "all") return;
    const value = Number(discountDraft);
    if (!Number.isFinite(value) || value <= 0 || value > 100) {
      toast.push({ variant: "error", title: "打折百分比必须大于 0 且不超过 100" });
      return;
    }
    try {
      await apiPutJson(`/v1/admin/model-vendor-discounts/${encodeURIComponent(appTab)}`, { discount_percent: discountDraft });
      toast.push({ variant: "success", title: "厂商折扣已保存" });
      reload();
    } catch (err) {
      toast.push({ variant: "error", title: "保存失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <div className="grid gap-4">
      <div className="flex justify-end">
        <button onClick={() => { setEditTarget(null); setEditorOpen(true); }} className="rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop">新增模型</button>
      </div>
      <div className="flex flex-wrap gap-2">
        {["all", ...appTypes].map((app) => (
          <button
            key={app}
            type="button"
            onClick={() => setAppTab(app)}
            className={`rounded-full border-2 border-slate-800 px-4 py-2 text-sm font-extrabold uppercase lift ${appTab === app ? "bg-violet-500 text-white" : "bg-white"}`}
          >
            {app === "all" ? "全部" : app}
            <span className="ml-2 rounded-full bg-amber-300 px-2 py-0.5 text-xs text-slate-900">
              {app === "all" ? (items ?? []).length : (items ?? []).filter((item) => String(item.app_type ?? "") === app).length}
            </span>
          </button>
        ))}
      </div>
      {appTab !== "all" && (
        <div className="flex flex-wrap items-end gap-3 rounded-lg border-2 border-slate-800 bg-white p-3">
          <Field label={`${appTab} 打折百分比`}>
            <input
              value={discountDraft}
              onChange={(e) => setDiscountDraft(e.target.value)}
              inputMode="decimal"
              className="w-36 rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white"
            />
          </Field>
          <button onClick={saveDiscount} className="rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop">保存折扣</button>
          <div className="pb-2 text-sm font-semibold text-slate-600">表格价格 = 官方价格 × 打折百分比</div>
        </div>
      )}
      <AdminDataTable
        rows={rows}
        loading={items === null}
        rowKey={(r, i) => String(r.id ?? i)}
        empty={<EmptyState shape="triangle" title="尚未配置模型" hint="新增模型并设置价格后才能允许 API 用户调用" />}
        columns={[
          { key: "app", header: "类型", render: (r) => <span className="rounded-full bg-violet-100 border-2 border-slate-800 px-2 py-0.5 text-xs font-bold uppercase">{String(r.app_type ?? "")}</span> },
          { key: "pattern", header: "模型", render: (r) => <span className="font-mono text-sm">{String(r.model_pattern ?? "")}</span> },
          { key: "input", header: "输入 / 1M", render: (r) => <ModelPriceCell row={r} priceKey="input_per_million" officialKey="official_input_per_million" /> },
          { key: "output", header: "输出 / 1M", render: (r) => <ModelPriceCell row={r} priceKey="output_per_million" officialKey="official_output_per_million" /> },
          { key: "cacheRead", header: "缓存/读", render: (r) => <ModelPriceCell row={r} priceKey="cache_read_per_million" officialKey="official_cache_read_per_million" /> },
          { key: "cacheWrite", header: "缓存/写", render: (r) => <ModelPriceCell row={r} priceKey="cache_write_per_million" officialKey="official_cache_write_per_million" /> },
          { key: "status", header: "状态", render: (r) => <Pill status={String(r.status ?? "active")} /> },
          { key: "actions", header: "操作", render: (r) => (
            <div className="flex flex-wrap gap-2">
              <button onClick={() => { setEditTarget(r); setEditorOpen(true); }} className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift">编辑</button>
              <button onClick={() => toggleStatus(r)} className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift">{String(r.status ?? "active") === "active" ? "下线" : "上线"}</button>
              {String(r.status ?? "active") !== "active" && (
                <button onClick={() => deleteModel(r)} className="rounded-full border-2 border-slate-800 bg-pink-100 px-3 py-1 text-xs font-bold lift">删除</button>
              )}
            </div>
          ) }
        ]}
      />
      <ModelEditorModal open={editorOpen} target={editTarget} onClose={() => setEditorOpen(false)} onDone={() => { reload(); setEditorOpen(false); }} />
    </div>
  );
}

function compareModelRows(a: AnyRow, b: AnyRow) {
  const activeDelta = (String(b.status ?? "active") === "active" ? 1 : 0) - (String(a.status ?? "active") === "active" ? 1 : 0);
  if (activeDelta !== 0) return activeDelta;
  for (const key of ["output_per_million", "input_per_million", "cache_write_per_million", "cache_read_per_million"]) {
    const delta = Number(nestedPrice(b, key) ?? 0) - Number(nestedPrice(a, key) ?? 0);
    if (delta !== 0) return delta;
  }
  return String(a.model_pattern ?? "").localeCompare(String(b.model_pattern ?? ""));
}

function ModelPriceCell({ row, priceKey, officialKey }: { row: AnyRow; priceKey: string; officialKey: string }) {
  const price = row.price as AnyRow | undefined;
  const effective = price?.[priceKey];
  const official = price?.[officialKey];
  const discount = price?.discount_percent ?? "10";
  if (effective === null || effective === undefined) return <span className="text-slate-400">-</span>;
  return (
    <div className="grid gap-0.5">
      <span className="font-mono">{formatAdminPrice(effective)}</span>
      <span className="text-[11px] font-semibold text-slate-500">官方 {formatAdminPrice(official ?? effective)} × {formatCompactNumber(discount)}%</span>
    </div>
  );
}

function nestedPrice(row: AnyRow, key: string) {
  const price = row.price as AnyRow | undefined;
  return price?.[key] ?? "0";
}

function formatCompactNumber(value: unknown): string {
  const amount = Number(value ?? 0);
  if (!Number.isFinite(amount)) return "0";
  return amount.toFixed(4).replace(/0+$/, "").replace(/\.$/, "");
}

function formatAdminUsd(value: unknown): string {
  const amount = Number(value ?? 0);
  if (!Number.isFinite(amount) || amount === 0) return "$0.00";
  const sign = amount < 0 ? "-" : "";
  const abs = Math.abs(amount);
  if (abs >= 0.01) return `${sign}$${abs.toFixed(2)}`;
  return `${sign}$${abs.toFixed(8).replace(/0+$/, "").replace(/\.$/, ".00")}`;
}

function formatAdminPrice(value: unknown): string {
  const text = String(value ?? "0");
  if (!text.includes(".")) return `$${text}`;
  return `$${text.replace(/(\.\d*?[1-9])0+$/, "$1").replace(/\.0+$/, "")}`;
}

function ModelEditorModal({ open, target, onClose, onDone }: { open: boolean; target: AnyRow | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [knownApps, setKnownApps] = useState<string[]>([]);
  const [appMode, setAppMode] = useState<"known" | "custom">("known");
  const [form, setForm] = useState({
    id: "", app_type: "openai", model_pattern: "*",
    input_per_million: "0", output_per_million: "0",
    cache_read_per_million: "0", cache_write_per_million: "0",
    status: "active", sort_order: "0", display_name: ""
  });
  useEffect(() => {
    if (open) {
      apiGet<AnyRow[]>("/v1/admin/models")
        .then((rows) => setKnownApps(Array.from(new Set(rows.map((row) => String(row.app_type ?? "")).filter(Boolean))).sort()))
        .catch(() => setKnownApps(["anthropic", "deepseek", "gemini", "openai"]));
    }
    if (target) {
      const price = target.price as AnyRow | undefined;
      setForm({
        id: String(target.id ?? ""),
        app_type: String(target.app_type ?? "openai"),
        model_pattern: String(target.model_pattern ?? "*"),
        display_name: String(target.display_name ?? ""),
        input_per_million: String(price?.official_input_per_million ?? price?.input_per_million ?? "0"),
        output_per_million: String(price?.official_output_per_million ?? price?.output_per_million ?? "0"),
        cache_read_per_million: String(price?.official_cache_read_per_million ?? price?.cache_read_per_million ?? "0"),
        cache_write_per_million: String(price?.official_cache_write_per_million ?? price?.cache_write_per_million ?? "0"),
        status: String(target.status ?? "active"),
        sort_order: String(target.sort_order ?? "0"),
      });
      setAppMode(["anthropic", "deepseek", "gemini", "openai"].includes(String(target.app_type ?? "")) ? "known" : "custom");
    } else if (open) {
      setForm({ id: "", app_type: "openai", model_pattern: "*", display_name: "", input_per_million: "0", output_per_million: "0", cache_read_per_million: "0", cache_write_per_million: "0", status: "active", sort_order: "0" });
      setAppMode("known");
    }
  }, [open, target]);
  async function submit() {
    try {
      let id = form.id;
      const metadata = {
        app_type: form.app_type,
        model_pattern: form.model_pattern,
        display_name: form.display_name || null,
        status: form.status,
        sort_order: Number(form.sort_order || 0)
      };
      if (id) {
        await apiPatchJson(`/v1/admin/models/${id}`, metadata);
      } else {
        const created = await apiPost<AnyRow>("/v1/admin/models", {
          ...metadata,
          input_per_million: form.input_per_million,
          output_per_million: form.output_per_million,
          cache_read_per_million: form.cache_read_per_million,
          cache_write_per_million: form.cache_write_per_million
        });
        id = String(created.id);
      }
      if (id) await apiPutJson(`/v1/admin/models/${id}/price`, {
        input_per_million: form.input_per_million, output_per_million: form.output_per_million,
        cache_read_per_million: form.cache_read_per_million, cache_write_per_million: form.cache_write_per_million
      });
      toast.push({ variant: "success", title: "已保存模型" });
      onDone();
    } catch (err) {
      toast.push({ variant: "error", title: "保存失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={open} onClose={onClose} title={target ? "编辑模型" : "新增模型"} width="lg">
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="App 类型">
          <div className="grid gap-2">
            <select
              value={appMode === "custom" ? "__custom" : form.app_type}
              onChange={(e) => {
                if (e.target.value === "__custom") {
                  setAppMode("custom");
                  setForm({ ...form, app_type: "" });
                } else {
                  setAppMode("known");
                  setForm({ ...form, app_type: e.target.value });
                }
              }}
              className="w-full rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 font-bold"
            >
              {Array.from(new Set(["openai", "anthropic", "gemini", "deepseek", ...knownApps])).map((app) => <option key={app} value={app}>{app}</option>)}
              <option value="__custom">自定义...</option>
            </select>
            {appMode === "custom" && (
              <input
                value={form.app_type}
                onChange={(e) => setForm({ ...form, app_type: e.target.value.toLowerCase() })}
                placeholder="例如 codex 或 openrouter"
                className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white"
              />
            )}
          </div>
        </Field>
        <Field label="模型 Pattern"><input value={form.model_pattern} onChange={(e) => setForm({ ...form, model_pattern: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="展示名"><input value={form.display_name} onChange={(e) => setForm({ ...form, display_name: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 outline-none focus:bg-white" /></Field>
        <Field label="排序"><input value={form.sort_order} onChange={(e) => setForm({ ...form, sort_order: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="官方输入 / 1M"><input value={form.input_per_million} onChange={(e) => setForm({ ...form, input_per_million: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="官方输出 / 1M"><input value={form.output_per_million} onChange={(e) => setForm({ ...form, output_per_million: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="官方缓存读 / 1M"><input value={form.cache_read_per_million} onChange={(e) => setForm({ ...form, cache_read_per_million: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="官方缓存写 / 1M"><input value={form.cache_write_per_million} onChange={(e) => setForm({ ...form, cache_write_per_million: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="状态"><select value={form.status} onChange={(e) => setForm({ ...form, status: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 font-bold">
          <option value="active">active</option><option value="inactive">inactive</option>
        </select></Field>
        <Field label="折后输入 / 1M"><div className="rounded-2xl border-2 border-slate-800 bg-slate-50 px-3 py-2 font-mono">{formatAdminPrice(discountedModelPrice(form.input_per_million, target))}</div></Field>
        <Field label="折后输出 / 1M"><div className="rounded-2xl border-2 border-slate-800 bg-slate-50 px-3 py-2 font-mono">{formatAdminPrice(discountedModelPrice(form.output_per_million, target))}</div></Field>
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">保存</button>
      </ModalActions>
    </Modal>
  );
}

function discountedModelPrice(value: unknown, target: AnyRow | null): string {
  const official = Number(value ?? 0);
  const discount = Number((target?.price as AnyRow | undefined)?.discount_percent ?? 10);
  if (!Number.isFinite(official) || !Number.isFinite(discount)) return "0";
  return String((official * discount) / 100);
}

function ModelRoutingModal({ open, target, onClose, onDone }: { open: boolean; target: AnyRow | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [shares, setShares] = useState<AnyRow[]>([]);
  const routing = target?.routing as AnyRow | undefined;
  const existing = (routing?.shares as AnyRow[] | undefined) ?? [];
  const [mode, setMode] = useState("all");
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [previewModel, setPreviewModel] = useState("");
  const [preview, setPreview] = useState<AnyRow | null>(null);
  useEffect(() => {
    if (!open) return;
    apiGetAllItems<AnyRow>("/v1/admin/shares").then(setShares).catch(() => setShares([]));
    setMode(String(routing?.mode ?? "all"));
    setSelected(new Set(existing.map((s) => `${String(s.router_id)}:${String(s.share_id)}`)));
    setPreviewModel(String(target?.model_pattern ?? "").replace(/\*$/, ""));
    setPreview(null);
  }, [open, target]);
  function toggle(key: string) {
    const next = new Set(selected);
    if (next.has(key)) next.delete(key); else next.add(key);
    setSelected(next);
  }
  async function submit() {
    if (!target?.id) return;
    try {
      const id = String(target.id);
      await apiPutJson(`/v1/admin/models/${id}/routing`, { mode, enabled: true });
      await apiPutJson(`/v1/admin/models/${id}/routing/shares`, {
        shares: shares.filter((s) => selected.has(`${String(s.router_id)}:${String(s.share_id)}`)).map((s) => ({ router_id: String(s.router_id), share_id: String(s.share_id) }))
      });
      toast.push({ variant: "success", title: "已保存路由规则" });
      onDone();
    } catch (err) {
      toast.push({ variant: "error", title: "保存失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  async function runPreview() {
    if (!target) return;
    try {
      const value = await apiPost<AnyRow>("/v1/admin/models/route-preview", {
        app_type: String(target.app_type),
        model: previewModel || String(target.model_pattern ?? "").replace(/\*$/, "")
      });
      setPreview(value);
    } catch (err) {
      toast.push({ variant: "error", title: "预览失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  const diagnostics = preview?.diagnostics as AnyRow | undefined;
  return (
    <Modal open={open} onClose={onClose} title="模型 Share 路由" description={target ? `${String(target.model_pattern)} · 默认跟随 ForSale 和 client capability 自动路由` : ""} width="lg">
      <div className="grid gap-4">
        <div className="rounded-3xl border-2 border-slate-800 bg-emerald-50 p-4 text-sm text-slate-700">
          默认模式会自动使用已 ForSale 且 client 声明支持对应 Claude/Codex/Gemini 的 share。Market 黑名单在 Shares 页维护，优先级高于自动路由和手动绑定
        </div>
        <Field label="路由模式"><select value={mode} onChange={(e) => setMode(e.target.value)} className="w-full rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 font-bold">
          <option value="all">自动路由（推荐）</option>
          <option value="exclude">排除选中 share</option>
          <option value="include_only">只允许选中 share</option>
        </select></Field>
        <div className="max-h-80 overflow-auto rounded-3xl border-2 border-slate-800 bg-white p-3">
          {shares.length === 0 ? <EmptyState shape="circle" title="暂无 share" hint="先在 Shares tab 同步 router shares" /> : shares.map((s) => {
            const key = `${String(s.router_id)}:${String(s.share_id)}`;
            return (
              <label key={key} className="mb-2 flex items-center gap-3 rounded-2xl border-2 border-slate-200 bg-amber-50 px-3 py-2 text-sm">
                <input type="checkbox" checked={selected.has(key)} onChange={() => toggle(key)} />
                <span className="font-mono text-xs">{String(s.share_id)}</span>
                <span className="ml-auto text-xs text-slate-600">{String(s.owner_email ?? s.installation_owner_email ?? "")}</span>
              </label>
            );
          })}
        </div>
        <div className="rounded-3xl border-2 border-slate-800 bg-emerald-50 p-4">
          <div className="flex flex-wrap items-end gap-3">
            <Field label="路由预览模型"><input value={previewModel} onChange={(e) => setPreviewModel(e.target.value)} className="w-80 max-w-full rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 font-mono outline-none" /></Field>
            <button onClick={runPreview} className="rounded-full border-2 border-slate-800 bg-white px-4 py-2 text-sm font-bold lift">预览</button>
          </div>
          {diagnostics && (
            <div className="mt-3 grid gap-2 text-xs sm:grid-cols-3">
              <PreviewCount label="最终候选" value={(diagnostics.final_candidates as unknown[] | undefined)?.length ?? 0} />
              <PreviewCount label="并发排除" value={(diagnostics.excluded_parallel_limit as unknown[] | undefined)?.length ?? 0} />
              <PreviewCount label="冷却排除" value={(diagnostics.excluded_cooldown as unknown[] | undefined)?.length ?? 0} />
              <PreviewCount label="规则排除" value={(diagnostics.excluded_by_rule as unknown[] | undefined)?.length ?? 0} />
              <PreviewCount label="Blocklist" value={(diagnostics.excluded_blocklist as unknown[] | undefined)?.length ?? 0} />
              <PreviewCount label="不可用" value={(diagnostics.excluded_offline as unknown[] | undefined)?.length ?? 0} />
              <div className="sm:col-span-3 rounded-2xl border-2 border-slate-800 bg-white p-3">
                <div className="font-bold">当前选中</div>
                <pre className="mt-1 overflow-auto text-[11px]">{JSON.stringify(diagnostics.selected_share ?? null, null, 2)}</pre>
              </div>
            </div>
          )}
        </div>
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">保存</button>
      </ModalActions>
    </Modal>
  );
}

function PreviewCount({ label, value }: { label: string; value: number }) {
  return (
    <div className="rounded-2xl border-2 border-slate-800 bg-white p-3">
      <div className="font-bold text-slate-500">{label}</div>
      <div className="font-display text-2xl font-extrabold">{value}</div>
    </div>
  );
}

function Field({ label, children, full }: { label: string; children: ReactNode; full?: boolean }) {
  return (
    <label className={`grid gap-1 ${full ? "sm:col-span-2" : ""}`}>
      <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{label}</span>
      {children}
    </label>
  );
}

function SharesTab() {
  const toast = useToast();
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const [syncing, setSyncing] = useState(false);
  const [updatingKey, setUpdatingKey] = useState<string | null>(null);
  const formatDate = useDateTimeFormatter();
  function reload() { apiGetAllItems<AnyRow>("/v1/admin/shares").then(setItems).catch(() => setItems([])); }
  useEffect(() => { reload(); }, []);
  async function sync() {
    setSyncing(true);
    try {
      const value = await apiPost<{ synced?: number }>("/v1/admin/shares/sync", {});
      toast.push({ variant: "success", title: `已同步 ${value.synced ?? 0} 个 share` });
      reload();
    } catch (err) {
      toast.push({ variant: "error", title: "同步失败", description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSyncing(false);
    }
  }
  async function setCapabilityBlocked(row: AnyRow, capability: string, blocked: boolean) {
    const routerId = String(row.router_id ?? "");
    const shareId = String(row.share_id ?? "");
    if (!routerId || !shareId) return;
    const key = `${routerId}:${shareId}:${capability}`;
    setUpdatingKey(key);
    try {
      if (blocked) {
        await apiPost("/v1/admin/share-capability-blocks", {
          router_id: routerId,
          share_id: shareId,
          capability,
          reason: "admin disabled capability"
        });
      } else {
        await apiDelete(`/v1/admin/share-capability-blocks/${encodeURIComponent(routerId)}/${encodeURIComponent(shareId)}/${encodeURIComponent(capability)}`);
      }
      toast.push({ variant: "success", title: blocked ? "已加入黑名单" : "已解除黑名单" });
      reload();
    } catch (err) {
      toast.push({ variant: "error", title: "更新失败", description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setUpdatingKey(null);
    }
  }
  return (
    <div className="grid gap-4">
      <div className="flex justify-end">
        <button onClick={sync} disabled={syncing} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop disabled:opacity-50">
          <RotateCw size={16} className={syncing ? "animate-spin" : ""} /> 同步 Router Shares
        </button>
      </div>
      <AdminDataTable
        rows={items ?? []}
        loading={items === null}
        rowKey={(r, i) => String(r.share_id ?? i)}
        empty={<EmptyState shape="circle" title="暂无 share 缓存" hint="点击「同步 Router Shares」从 router 拉取" />}
        columns={[
          { key: "share", header: "Share", render: (r) => <span className="font-mono text-xs break-all">{String(r.share_id ?? "")}</span> },
          { key: "owner", header: "Owner", render: (r) => <span className="font-mono text-sm">{String(r.owner_email ?? r.installation_owner_email ?? "")}</span> },
          { key: "app", header: "类型", render: (r) => <span className="rounded-full bg-violet-100 border-2 border-slate-800 px-2 py-0.5 text-xs font-bold uppercase">{String(r.app_type ?? "")}</span> },
          { key: "support", header: "Client 支持", render: (r) => <ShareCapabilityBadges row={r} mode="support" /> },
          { key: "models", header: "模型支持", render: (r) => <ShareModelSupport row={r} /> },
          { key: "blocked", header: "Market 黑名单", render: (r) => <ShareCapabilityBadges row={r} mode="blocked" /> },
          { key: "online", header: "在线", render: (r) => <Pill variant={r.online ? "success" : "neutral"}>{r.online ? "在线" : "离线"}</Pill> },
          { key: "load", header: "并发", render: (r) => <span className="font-mono text-sm">{String(r.active_requests ?? 0)} / {String(r.parallel_limit ?? 0)}</span> },
          { key: "seen", header: "最近同步", render: (r) => <span className="text-xs text-slate-500">{formatDate(String(r.last_seen_at ?? ""))}</span> },
          { key: "actions", header: "操作", render: (r) => <ShareCapabilityActions row={r} updatingKey={updatingKey} onToggle={setCapabilityBlocked} /> }
        ]}
      />
    </div>
  );
}

const SHARE_CAPABILITIES = [
  { key: "codex", label: "Codex" },
  { key: "claude", label: "Claude" },
  { key: "gemini", label: "Gemini" }
];

function ShareCapabilityBadges({ row, mode }: { row: AnyRow; mode: "support" | "blocked" }) {
  const items = SHARE_CAPABILITIES.filter((cap) => Boolean(row[mode === "support" ? `enabled_${cap.key}` : `blocked_${cap.key}`]));
  if (items.length === 0) return <span className="text-xs text-slate-400">—</span>;
  return (
    <div className="flex flex-wrap gap-1">
      {items.map((cap) => (
        <span key={cap.key} className={`rounded-full border-2 border-slate-800 px-2 py-0.5 text-[11px] font-bold ${mode === "support" ? "bg-emerald-100 text-emerald-800" : "bg-pink-100 text-pink-700"}`}>{cap.label}</span>
      ))}
    </div>
  );
}

function ShareModelSupport({ row }: { row: AnyRow }) {
  const raw = parseRawJson(row.raw_json);
  const runtimes = (raw?.appRuntimes ?? raw?.app_runtimes ?? {}) as Record<string, AnyRow | undefined>;
  const lines = ["claude", "codex", "gemini"].map((app) => {
    const runtime = runtimes?.[app] as AnyRow | undefined;
    if (!runtime) return null;
    if (String(runtime.kind ?? "") === "official_oauth") return `${app}: official`;
    const models = Array.isArray(runtime.models) ? runtime.models : [];
    const text = models
      .map((item: AnyRow) => `${String(item.slot ?? "model")}:${String(item.actualModel ?? item.actual_model ?? "")}`)
      .filter((item: string) => !item.endsWith(":"))
      .join(" . ");
    return text ? `${app}: ${text}` : null;
  }).filter(Boolean);
  if (lines.length === 0) return <span className="text-xs text-slate-400">—</span>;
  return <div className="grid gap-1 text-[11px] font-mono">{lines.map((line) => <div key={line}>{line}</div>)}</div>;
}

function parseRawJson(value: unknown): AnyRow | null {
  if (!value) return null;
  if (typeof value === "object") return value as AnyRow;
  if (typeof value !== "string") return null;
  try {
    return JSON.parse(value) as AnyRow;
  } catch {
    return null;
  }
}

function ShareCapabilityActions({ row, updatingKey, onToggle }: { row: AnyRow; updatingKey: string | null; onToggle: (row: AnyRow, capability: string, blocked: boolean) => void }) {
  const routerId = String(row.router_id ?? "");
  const shareId = String(row.share_id ?? "");
  return (
    <div className="flex flex-wrap gap-2">
      {SHARE_CAPABILITIES.map((cap) => {
        const supported = Boolean(row[`enabled_${cap.key}`]);
        const blocked = Boolean(row[`blocked_${cap.key}`]);
        const key = `${routerId}:${shareId}:${cap.key}`;
        return (
          <button
            key={cap.key}
            disabled={!supported || updatingKey === key}
            onClick={() => onToggle(row, cap.key, !blocked)}
            className={`rounded-full border-2 border-slate-800 px-3 py-1 text-xs font-bold lift disabled:cursor-not-allowed disabled:opacity-40 ${blocked ? "bg-emerald-100 text-emerald-800" : "bg-pink-100 text-pink-700"}`}
            title={!supported ? "client 未声明支持该能力" : undefined}
          >
            {blocked ? `解除 ${cap.label}` : `禁用 ${cap.label}`}
          </button>
        );
      })}
    </div>
  );
}

function ChargesTab() {
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const [settleTarget, setSettleTarget] = useState<AnyRow | null>(null);
  const [releaseTarget, setReleaseTarget] = useState<AnyRow | null>(null);
  const [detailTarget, setDetailTarget] = useState<AnyRow | null>(null);
  const formatDate = useDateTimeFormatter();
  function reload() { apiGetAllItems<AnyRow>("/v1/admin/charges").then(setItems).catch(() => setItems([])); }
  useEffect(() => { reload(); }, []);
  const sortedItems = (items ?? []).slice().sort(compareChargeRows);
  return (
    <div className="grid gap-4">
      <AdminDataTable
        rows={sortedItems}
        loading={items === null}
        rowKey={(r, i) => String(r.id ?? i)}
        rowClassName={(r) => {
          const status = String(r.status);
          if (status === "needs_review") return "bg-pink-50";
          if (status === "reserved") return "bg-violet-50";
          return "";
        }}
        empty={<EmptyState shape="circle" title="还没有计费记录" />}
        columns={[
          { key: "rid", header: "Request", render: (r) => <span className="font-mono text-xs break-all">{String(r.request_id ?? "")}</span> },
          { key: "email", header: "Email", render: (r) => <span className="font-mono text-xs break-all">{String(r.requester_email ?? "")}</span> },
          { key: "share", header: "SHARE", render: (r) => <span className="font-mono text-xs">{String(r.share_subdomain ?? "")}</span> },
          { key: "agent", header: "Agent", render: (r) => <span className="rounded-full bg-violet-100 border-2 border-slate-800 px-2 py-0.5 text-xs font-bold uppercase">{String(r.request_agent ?? r.app_type ?? "")}</span> },
          { key: "requested", header: "请求模型", render: (r) => <span className="font-mono text-xs break-all">{String(r.requested_model ?? r.model ?? "")}</span> },
          { key: "actual", header: "真实模型", render: (r) => <span className="font-mono text-xs break-all">{String(r.actual_model ?? r.pricing_model ?? r.model ?? "")}</span> },
          { key: "amount", header: "金额", render: (r) => <span className="font-mono">{r.usage_amount ? `$${r.usage_amount}` : `锁定 $${r.reserved_amount ?? "0"}`}</span> },
          { key: "status", header: "状态", render: (r) => <Pill status={String(r.status ?? "")} /> },
          { key: "time", header: "时间", render: (r) => <span className="text-xs text-slate-500">{formatDate(String(r.created_at ?? ""))}</span> },
          {
            key: "audit",
            header: "审计标记",
            className: "w-64 max-w-64",
            render: (r) => r.audit_flags
              ? <span className="block whitespace-normal break-words text-xs text-pink-700">{String(r.audit_flags)}</span>
              : <span className="text-xs text-slate-400">—</span>
          },
          { key: "actions", header: "操作", render: (r) =>
            String(r.status) === "needs_review" ? (
              <div className="flex flex-wrap gap-2">
                <button onClick={() => setDetailTarget(r)} className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift">详情</button>
                <button onClick={() => setSettleTarget(r)} className="rounded-full border-2 border-slate-800 bg-emerald-200 px-3 py-1 text-xs font-bold lift">手动结算</button>
                <button onClick={() => setReleaseTarget(r)} className="rounded-full border-2 border-slate-800 bg-pink-200 px-3 py-1 text-xs font-bold lift">释放</button>
              </div>
            ) : <span className="text-xs text-slate-400">—</span>
          }
        ]}
      />
      <ChargeReviewDetailModal target={detailTarget} onClose={() => setDetailTarget(null)} />
      <SettleManualModal target={settleTarget} onClose={() => setSettleTarget(null)} onDone={reload} />
      <ReleaseChargeModal target={releaseTarget} onClose={() => setReleaseTarget(null)} onDone={reload} />
    </div>
  );
}

function compareChargeRows(a: AnyRow, b: AnyRow) {
  const priorityDelta = chargeStatusPriority(a) - chargeStatusPriority(b);
  if (priorityDelta !== 0) return priorityDelta;
  return rowTimeDesc(a, b);
}

function chargeStatusPriority(row: AnyRow) {
  const status = String(row.status);
  if (status === "needs_review") return 0;
  if (status === "reserved") return 1;
  return 2;
}

function rowTimeDesc(a: AnyRow, b: AnyRow) {
  return rowTimeMs(b) - rowTimeMs(a);
}

function rowTimeMs(row: AnyRow) {
  const value = String(row.created_at ?? row.settled_at ?? "");
  const parsed = Date.parse(value);
  return Number.isFinite(parsed) ? parsed : 0;
}

function ChargeReviewDetailModal({ target, onClose }: { target: AnyRow | null; onClose: () => void }) {
  const toast = useToast();
  const formatDate = useDateTimeFormatter();
  const [detail, setDetail] = useState<AnyRow | null>(null);
  const [loading, setLoading] = useState(false);
  useEffect(() => {
    if (!target?.id) {
      setDetail(null);
      return;
    }
    setLoading(true);
    apiGet<AnyRow>(`/v1/admin/charges/${String(target.id)}/review-context`)
      .then(setDetail)
      .catch(() => setDetail(null))
      .finally(() => setLoading(false));
  }, [target?.id]);

  async function copyText(value: unknown) {
    const text = String(value ?? "");
    if (!text || typeof navigator === "undefined" || !navigator.clipboard) return;
    try {
      await navigator.clipboard.writeText(text);
      toast.push({ variant: "success", title: "已复制" });
    } catch (err) {
      toast.push({ variant: "error", title: "复制失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }

  const charge = (detail?.charge as AnyRow | undefined) ?? target ?? {};
  const attempts = Array.isArray(detail?.attempts) ? detail.attempts as AnyRow[] : [];
  const curl = (detail?.curl as AnyRow | undefined) ?? {};
  const requestObject = (detail?.requestObject as AnyRow | undefined) ?? {};
  const responseMetaObject = (detail?.responseMetaObject as AnyRow | undefined) ?? {};
  const routerShare = detail?.routerShare as AnyRow | null | undefined;
  const auditFlags = parseJsonArray(charge.audit_flags);

  return (
    <Modal open={!!target} onClose={onClose} title="待复核详情" description={target ? `Request ${String(target.request_id ?? "")}` : ""} width="lg">
      {loading ? (
        <div className="grid gap-3">
          <Skeleton className="h-16 rounded-2xl" />
          <Skeleton className="h-40 rounded-2xl" />
        </div>
      ) : (
        <div className="grid gap-4">
          <div className="grid gap-3 md:grid-cols-3">
            <ReviewInfo label="状态" value={<Pill status={String(charge.status ?? "")} />} />
            <ReviewInfo label="Agent" value={<span className="font-mono">{String(charge.request_agent ?? charge.app_type ?? "")}</span>} />
            <ReviewInfo label="请求模型" value={<span className="font-mono">{String(charge.requested_model ?? charge.model ?? "")}</span>} />
            <ReviewInfo label="真实模型" value={<span className="font-mono">{String(charge.actual_model ?? charge.pricing_model ?? charge.model ?? "")}</span>} />
            <ReviewInfo label="创建时间" value={formatDate(String(charge.created_at ?? ""))} />
            <ReviewInfo label="调用用户" value={<span className="font-mono break-all">{String(charge.requester_email ?? "")}</span>} />
            <ReviewInfo label="Share" value={<span className="font-mono break-all">{String(charge.share_subdomain ?? "")}</span>} />
            <ReviewInfo label="预授权" value={`$${String(charge.reserved_amount ?? "0")}`} />
            <ReviewInfo label="实际费用" value={charge.usage_amount ? `$${String(charge.usage_amount)}` : "—"} />
            <ReviewInfo label="Share ID" value={<span className="font-mono break-all">{String(charge.share_id ?? "")}</span>} />
          </div>

          <ReviewSection title="待复核原因">
            <div className="flex flex-wrap gap-2">
              {auditFlags.length ? auditFlags.map((flag) => (
                <span key={flag} className="rounded-full border-2 border-slate-800 bg-pink-100 px-3 py-1 text-xs font-bold text-pink-700">{flag}</span>
              )) : <span className="text-sm text-slate-500">没有审计标记</span>}
            </div>
            <p className="mt-2 text-sm text-slate-600">{reviewReasonHint(auditFlags)}</p>
          </ReviewSection>

          <ReviewSection title="路由上下文">
            <JsonBlock value={{ routerId: charge.router_id, shareId: charge.share_id, ownerEmail: charge.owner_email, routerShare }} />
          </ReviewSection>

          <ReviewSection title="请求尝试">
            <JsonBlock value={attempts} />
          </ReviewSection>

          <ReviewSection title="Request Object">
            <ObjectHeader object={requestObject} />
            <JsonBlock value={requestObject.json ?? null} />
          </ReviewSection>

          <ReviewSection title="Response Meta">
            <ObjectHeader object={responseMetaObject} />
            {responseMetaObject.json ? <JsonBlock value={responseMetaObject.json} /> : <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 p-4 text-sm text-slate-600">没有 response meta，通常表示尚未成功解析 usage 或尚未完成结算</div>}
          </ReviewSection>

          <ReviewSection title="Linux curl 复现">
            <CurlBlock title="Market Replay" value={curl.marketReplay} onCopy={copyText} />
            <CurlBlock title="Share Replay" value={curl.shareReplay} onCopy={copyText} />
          </ReviewSection>
        </div>
      )}
    </Modal>
  );
}

function ReviewInfo({ label, value }: { label: string; value: ReactNode }) {
  return (
    <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 p-3">
      <div className="text-xs font-bold uppercase tracking-wider text-slate-500">{label}</div>
      <div className="mt-1 text-sm font-bold">{value}</div>
    </div>
  );
}

function ReviewSection({ title, children }: { title: string; children: ReactNode }) {
  return (
    <section className="grid gap-2">
      <h3 className="font-display text-lg font-extrabold">{title}</h3>
      {children}
    </section>
  );
}

function ObjectHeader({ object }: { object: AnyRow }) {
  return (
    <div className="grid gap-1 rounded-2xl border-2 border-slate-800 bg-white p-3 text-xs text-slate-600">
      <div>Object: <span className="font-mono break-all">{String(object.objectKey ?? "—")}</span></div>
      <div>SHA256: <span className="font-mono break-all">{String(object.sha256 ?? "—")}</span></div>
    </div>
  );
}

function JsonBlock({ value }: { value: unknown }) {
  return <pre className="max-h-96 overflow-auto rounded-2xl border-2 border-slate-800 bg-slate-900 p-3 text-xs text-emerald-200">{JSON.stringify(value, null, 2)}</pre>;
}

function CurlBlock({ title, value, onCopy }: { title: string; value: unknown; onCopy: (value: unknown) => void }) {
  if (!value) {
    return <div className="rounded-2xl border-2 border-slate-800 bg-slate-50 p-3 text-sm text-slate-500">{title}: 无法生成</div>;
  }
  return (
    <div className="grid gap-2 rounded-2xl border-2 border-slate-800 bg-white p-3">
      <div className="flex items-center justify-between gap-2">
        <div className="font-bold">{title}</div>
        <button onClick={() => onCopy(value)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-amber-100 px-3 py-1 text-xs font-bold lift"><Copy size={12} /> 复制</button>
      </div>
      <pre className="max-h-72 overflow-auto rounded-xl bg-slate-900 p-3 text-xs text-emerald-200">{String(value)}</pre>
    </div>
  );
}

function parseJsonArray(value: unknown): string[] {
  if (Array.isArray(value)) return value.map(String);
  if (typeof value !== "string") return [];
  try {
    const parsed = JSON.parse(value);
    return Array.isArray(parsed) ? parsed.map(String) : [];
  } catch {
    return value ? [value] : [];
  }
}

function reviewReasonHint(flags: string[]) {
  if (flags.includes("non_stream_usage_missing")) return "非流式请求返回成功，但响应中没有可解析的 usage。优先用 curl 复现，确认上游响应是否包含 usage";
  if (flags.includes("stream_usage_missing")) return "流式请求结束后没有拿到完整 usage。检查 SSE 尾部事件或客户端是否提前断开";
  if (flags.includes("stream_client_disconnected")) return "客户端在流式响应完成前断开，usage 可能尚未到达";
  if (flags.includes("stream_upstream_interrupted")) return "上游流中断，无法确认最终 usage";
  if (flags.includes("stream_settlement_failed")) return "已解析到 usage，但自动结算失败。检查价格快照、余额和账本";
  if (flags.includes("settlement_over_reserved")) return "实际费用超过预授权金额，需要复核是否补扣或释放";
  return "根据请求上下文、attempts 和 curl 复现结果决定手动结算或释放";
}

function SettleManualModal({ target, onClose, onDone }: { target: AnyRow | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [input, setInput] = useState("0");
  const [output, setOutput] = useState("0");
  const [reason, setReason] = useState("");
  useEffect(() => { if (target) { setInput("0"); setOutput("0"); setReason(""); } }, [target]);
  async function submit() {
    if (!target) return;
    if (!reason.trim()) { toast.push({ variant: "error", title: "请填写原因" }); return; }
    try {
      await apiPost(`/v1/admin/charges/${target.id}/settle-manual`, {
        input_tokens: Number(input), output_tokens: Number(output), reason: reason.trim()
      });
      toast.push({ variant: "success", title: "已结算" });
      onDone(); onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title="手动结算" description={target ? `Request ${String(target.request_id)}` : ""}>
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="输入 token"><input type="number" min="0" value={input} onChange={(e) => setInput(e.target.value)} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="输出 token"><input type="number" min="0" value={output} onChange={(e) => setOutput(e.target.value)} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
      </div>
      <textarea value={reason} onChange={(e) => setReason(e.target.value)} placeholder="结算原因 / 凭证描述" className="mt-3 min-h-24 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-emerald-400 px-5 py-2 font-bold text-white btn-pop">结算</button>
      </ModalActions>
    </Modal>
  );
}

function ReleaseChargeModal({ target, onClose, onDone }: { target: AnyRow | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [reason, setReason] = useState("");
  useEffect(() => { if (target) setReason(""); }, [target]);
  async function submit() {
    if (!target) return;
    if (!reason.trim()) { toast.push({ variant: "error", title: "请填写原因" }); return; }
    try {
      await apiPost(`/v1/admin/charges/${target.id}/release`, { reason: reason.trim() });
      toast.push({ variant: "success", title: "已释放预授权" });
      onDone(); onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title="释放预授权" description="将锁定金额退回 user_cash，不计入 client_payable">
      <textarea value={reason} onChange={(e) => setReason(e.target.value)} placeholder="释放原因" className="min-h-24 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-pink-400 px-5 py-2 font-bold text-white btn-pop">确认释放</button>
      </ModalActions>
    </Modal>
  );
}

function EarningsTab() {
  return <SimpleTable path="/v1/admin/earnings" empty="尚无 provider 应付汇总" />;
}

function MoneyWorkspaceTab() {
  const { locale } = useLocale();
  const c = copy[locale].admin.money;
  const localizedMoneyTabs: TabItem[] = [
    { key: "overview", label: c.tabs.overview, icon: <LayoutDashboard size={16} /> },
    { key: "events", label: c.tabs.events, icon: <Activity size={16} /> },
    { key: "charges", label: c.tabs.charges, icon: <Receipt size={16} /> },
    { key: "earnings", label: c.tabs.earnings, icon: <Coins size={16} /> },
    { key: "topups", label: c.tabs.topups, icon: <CreditCard size={16} /> },
    { key: "payouts", label: c.tabs.payouts, icon: <Banknote size={16} /> },
    { key: "ledger", label: c.tabs.ledger, icon: <BookOpen size={16} /> },
    { key: "check", label: c.tabs.check, icon: <ShieldCheck size={16} /> }
  ];
  return (
    <div className="grid gap-5">
      <div className="sticker bg-white p-5">
        <h2 className="font-display text-2xl font-extrabold">{c.workspaceTitle}</h2>
        <p className="mt-1 text-sm text-slate-600">{c.workspaceSubtitle}</p>
      </div>
      <Tabs items={localizedMoneyTabs} defaultKey="overview" storageKey="cc-switch-market:admin-money-tab">
        {(active) => {
          if (active === "overview") return <MoneyOverviewTab />;
          if (active === "events") return <MoneyEventsTab />;
          if (active === "charges") return <ChargesTab />;
          if (active === "earnings") return <EarningsTab />;
          if (active === "topups") return <TopupsTab />;
          if (active === "payouts") return <PayoutsTab />;
          if (active === "ledger") return <LedgerEntriesTab />;
          if (active === "check") return <LedgerCheckTab />;
          return null;
        }}
      </Tabs>
    </div>
  );
}

function MoneyOverviewTab() {
  const { locale } = useLocale();
  const c = copy[locale].admin.money.overview;
  const [overview, setOverview] = useState<AnyRow | null>(null);
  useEffect(() => { apiGet<AnyRow>("/v1/admin/money/overview").then(setOverview).catch(() => setOverview({ ledgerOk: false })); }, []);
  return (
    <div className="grid gap-4">
      <div className="grid gap-4 sm:grid-cols-2 lg:grid-cols-4">
        <StatCard label={c.todayTopups} value={formatAdminUsd(overview?.todayTopupsUsd)} color="violet" icon={<CreditCard size={16} />} loading={!overview} />
        <StatCard label={c.todayUsage} value={formatAdminUsd(overview?.todayUsageUsd)} color="pink" icon={<Receipt size={16} />} loading={!overview} />
        <StatCard label={c.providerPayable} value={formatAdminUsd(overview?.providerPayableUsd)} color="emerald" icon={<Coins size={16} />} loading={!overview} />
        <StatCard label={c.routerPayable} value={formatAdminUsd(overview?.routerPayableUsd)} sublabel={String(overview?.routerCommissionOwnerEmail ?? "")} color="emerald" icon={<Coins size={16} />} loading={!overview} />
        <StatCard label={c.payoutReserved} value={formatAdminUsd(overview?.payoutReservedUsd)} color="amber" icon={<Banknote size={16} />} loading={!overview} />
        <StatCard label={c.userCash} value={formatAdminUsd(overview?.userCashUsd)} color="violet" icon={<Users size={16} />} loading={!overview} />
        <StatCard label={c.userReserved} value={formatAdminUsd(overview?.userReservedUsd)} color="pink" icon={<Receipt size={16} />} loading={!overview} />
        <StatCard
          label={c.feeRevenue}
          value={formatAdminUsd(overview?.feeRevenueUsd)}
          sublabel={`${c.topupFees}: ${formatAdminUsd(overview?.topupFeeRevenueUsd)} · ${c.platformTakeRevenue}: ${formatAdminUsd(overview?.platformCommissionRevenueUsd)} · ${c.payoutFees}: ${formatAdminUsd(overview?.payoutFeeRevenueUsd)}`}
          color="emerald"
          icon={<Banknote size={16} />}
          loading={!overview}
        />
        <StatCard label={c.riskLoss} value={formatAdminUsd(overview?.riskLossUsd)} color="amber" icon={<AlertTriangle size={16} />} loading={!overview} />
      </div>
      <div className="grid gap-4 sm:grid-cols-3">
        <StatCard
          label={c.platformCommission}
          value={formatCommissionRate(overview?.platformCommissionBps)}
          sublabel={`${c.marketCommission}: ${formatCommissionRate(overview?.marketCommissionBps)} · ${c.routerCommission}: ${formatCommissionRate(overview?.routerCommissionBps)}`}
          color="amber"
          icon={<Receipt size={16} />}
          loading={!overview}
        />
        <StatCard label={c.pendingTopups} value={String(overview?.pendingTopups ?? 0)} color="violet" icon={<CreditCard size={16} />} loading={!overview} />
        <StatCard label={c.needsReview} value={String(overview?.needsReviewCharges ?? 0)} color="pink" icon={<AlertTriangle size={16} />} loading={!overview} />
        <StatCard label={c.pendingPayouts} value={String(overview?.pendingPayouts ?? 0)} color="emerald" icon={<Banknote size={16} />} loading={!overview} />
      </div>
      <div className={`sticker p-5 ${overview?.ledgerOk ? "bg-emerald-100" : "bg-pink-100"}`}>
        <div className="flex items-center justify-between gap-3">
          <div>
            <h3 className="font-display text-xl font-extrabold">{c.consistencyTitle}</h3>
            <p className="text-sm text-slate-700">{c.consistencyBody}</p>
          </div>
          <Pill variant={overview?.ledgerOk ? "success" : "failed"}>{overview?.ledgerOk ? c.consistencyOk : c.consistencyBad}</Pill>
        </div>
      </div>
    </div>
  );
}

function PayoutsTab() {
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const [executeTarget, setExecuteTarget] = useState<AnyRow | null>(null);
  const [paidTarget, setPaidTarget] = useState<AnyRow | null>(null);
  const [failedTarget, setFailedTarget] = useState<AnyRow | null>(null);
  const [cancelTarget, setCancelTarget] = useState<AnyRow | null>(null);

  function reload() { apiGetAllItems<AnyRow>("/v1/admin/payout-requests").then(setItems).catch(() => setItems([])); }
  useEffect(() => { reload(); }, []);

  return (
    <div className="grid gap-4">
      <AdminDataTable
        rows={items ?? []}
        loading={items === null}
        rowKey={(r, i) => String(r.id ?? i)}
        empty={<EmptyState shape="square" title="还没有提现请求" />}
        columns={[
          { key: "owner", header: "Owner", render: (r) => <span className="font-mono text-xs break-all">{String(r.owner_email ?? "")}</span> },
          { key: "amount", header: "金额", render: (r) => <MoneyAmount gross={String(r.amount_usd ?? "0")} fee={String(r.payout_fee_usd ?? "0")} net={String(r.net_payout_usd ?? "0")} layout="inline" /> },
          { key: "method", header: "方式", render: (r) => <span className="font-bold uppercase">{String(r.method ?? "")}</span> },
          { key: "status", header: "状态", render: (r) => <Pill status={String(r.status ?? "")} /> },
          { key: "external", header: "external tx", render: (r) => <span className="font-mono text-xs text-slate-500 break-all">{String(r.external_tx_id ?? r.gateio_batch_id ?? "—")}</span> },
          { key: "actions", header: "操作", render: (r) => {
            const status = String(r.status ?? "");
            const can = (...s: string[]) => s.includes(status);
            return (
              <div className="flex flex-wrap gap-2">
                {can("pending", "needs_review") && r.method === "gateio" && (
                  <button onClick={() => setExecuteTarget(r)} className="rounded-full border-2 border-slate-800 bg-emerald-200 px-3 py-1 text-xs font-bold lift">执行 Gate.io</button>
                )}
                {can("processing", "needs_review") && (
                  <button onClick={() => setPaidTarget(r)} className="rounded-full border-2 border-slate-800 bg-violet-200 px-3 py-1 text-xs font-bold lift">标记已付</button>
                )}
                {can("processing", "needs_review", "pending") && (
                  <button onClick={() => setFailedTarget(r)} className="rounded-full border-2 border-slate-800 bg-pink-200 px-3 py-1 text-xs font-bold lift">标记失败</button>
                )}
                {can("pending") && (
                  <button onClick={() => setCancelTarget(r)} className="rounded-full border-2 border-slate-800 bg-slate-100 px-3 py-1 text-xs font-bold lift">取消</button>
                )}
              </div>
            );
          } }
        ]}
      />
      <ConfirmModal target={executeTarget} onClose={() => setExecuteTarget(null)} title="执行 Gate.io 提现" reasonPlaceholder="备注（可选）" requireReason={false} confirmLabel="执行" onConfirm={async (r, reason) => { await apiPost(`/v1/admin/payout-requests/${r.id}/execute-gateio`, { reason }); reload(); }} />
      <PaidModal target={paidTarget} onClose={() => setPaidTarget(null)} onDone={reload} />
      <ReasonModal target={failedTarget} onClose={() => setFailedTarget(null)} title="标记失败" reasonPlaceholder="失败原因" confirmLabel="标记失败" onConfirm={async (r, reason) => { await apiPost(`/v1/admin/payout-requests/${r.id}/mark-failed`, { reason }); reload(); }} />
      <ConfirmModal target={cancelTarget} onClose={() => setCancelTarget(null)} title="取消提现" reasonPlaceholder="取消原因" requireReason confirmLabel="取消提现" onConfirm={async (r, reason) => { await apiPost(`/v1/admin/payout-requests/${r.id}/cancel`, { reason }); reload(); }} />
    </div>
  );
}

function PaidModal({ target, onClose, onDone }: { target: AnyRow | null; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [external, setExternal] = useState("");
  const [reason, setReason] = useState("");
  useEffect(() => { if (target) { setExternal(""); setReason(""); } }, [target]);
  async function submit() {
    if (!target) return;
    if (!external.trim()) { toast.push({ variant: "error", title: "请填写 external tx id" }); return; }
    if (!reason.trim()) { toast.push({ variant: "error", title: "请填写凭证说明" }); return; }
    try {
      await apiPost(`/v1/admin/payout-requests/${target.id}/mark-paid`, {
        external_tx_id: external.trim(), reason: reason.trim(), proof: { note: reason.trim() }
      });
      toast.push({ variant: "success", title: "已标记完成" });
      onDone(); onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title="标记提现已完成">
      {target && <MoneyAmount gross={String(target.amount_usd ?? "0")} fee={String(target.payout_fee_usd ?? "0")} net={String(target.net_payout_usd ?? "0")} layout="stacked" />}
      <input value={external} onChange={(e) => setExternal(e.target.value)} placeholder="external tx id" className="mt-4 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 font-mono outline-none focus:bg-white" />
      <textarea value={reason} onChange={(e) => setReason(e.target.value)} placeholder="凭证说明" className="mt-3 min-h-24 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-emerald-400 px-5 py-2 font-bold text-white btn-pop">确认完成</button>
      </ModalActions>
    </Modal>
  );
}

function ReasonModal({ target, onClose, title, reasonPlaceholder, confirmLabel, onConfirm }: { target: AnyRow | null; onClose: () => void; title: string; reasonPlaceholder: string; confirmLabel: string; onConfirm: (r: AnyRow, reason: string) => Promise<void> }) {
  const toast = useToast();
  const [reason, setReason] = useState("");
  useEffect(() => { if (target) setReason(""); }, [target]);
  async function submit() {
    if (!target) return;
    if (!reason.trim()) { toast.push({ variant: "error", title: "请填写原因" }); return; }
    try {
      await onConfirm(target, reason.trim());
      toast.push({ variant: "success", title: "已操作" });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title={title}>
      <textarea value={reason} onChange={(e) => setReason(e.target.value)} placeholder={reasonPlaceholder} className="min-h-24 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-pink-400 px-5 py-2 font-bold text-white btn-pop">{confirmLabel}</button>
      </ModalActions>
    </Modal>
  );
}

function ConfirmModal({ target, onClose, title, reasonPlaceholder, requireReason, confirmLabel, onConfirm }: { target: AnyRow | null; onClose: () => void; title: string; reasonPlaceholder: string; requireReason?: boolean; confirmLabel: string; onConfirm: (r: AnyRow, reason: string) => Promise<void> }) {
  const toast = useToast();
  const [reason, setReason] = useState("");
  useEffect(() => { if (target) setReason(""); }, [target]);
  async function submit() {
    if (!target) return;
    if (requireReason && !reason.trim()) { toast.push({ variant: "error", title: "请填写原因" }); return; }
    try {
      await onConfirm(target, reason.trim());
      toast.push({ variant: "success", title: "已执行" });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={!!target} onClose={onClose} title={title}>
      {target && <MoneyAmount gross={String(target.amount_usd ?? "0")} fee={String(target.payout_fee_usd ?? "0")} net={String(target.net_payout_usd ?? "0")} layout="stacked" />}
      <textarea value={reason} onChange={(e) => setReason(e.target.value)} placeholder={reasonPlaceholder} className="mt-4 min-h-24 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">{confirmLabel}</button>
      </ModalActions>
    </Modal>
  );
}

function TicketsTab() {
  const { locale } = useLocale();
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [statusFilter, setStatusFilter] = useState("all");
  const [priorityFilter, setPriorityFilter] = useState("all");
  const [typeFilter, setTypeFilter] = useState("all");
  const [waitingFilter, setWaitingFilter] = useState("all");
  const [keyword, setKeyword] = useState("");
  const [listScrollTop, setListScrollTop] = useState(0);
  const formatDate = useDateTimeFormatter();
  function reload() { apiGetAllItems<AnyRow>("/v1/admin/tickets").then(setItems).catch(() => setItems([])); }
  useEffect(() => { reload(); }, []);

  const sortedFilteredItems = (items ?? [])
    .filter((t) => {
      const status = String(t.status ?? "open");
      const priority = String(t.priority ?? "normal");
      const type = String(t.ticket_type ?? "");
      const waiting = String(t.waiting_for ?? "");
      const haystack = `${String(t.subject ?? "")} ${String(t.ticket_no ?? "")} ${String(t.creator_user_id ?? "")}`.toLowerCase();
      return (statusFilter === "all" || status === statusFilter)
        && (priorityFilter === "all" || priority === priorityFilter)
        && (typeFilter === "all" || type === typeFilter)
        && (waitingFilter === "all" || waiting === waitingFilter)
        && (!keyword.trim() || haystack.includes(keyword.trim().toLowerCase()));
    })
    .sort((a, b) => compareTicketCards(a, b));

  const activeIndex = sortedFilteredItems.findIndex((ticket) => String(ticket.id ?? "") === activeId);
  const activeTicket = activeIndex >= 0 ? sortedFilteredItems[activeIndex] : null;
  const prevTicketId = activeIndex > 0 ? String(sortedFilteredItems[activeIndex - 1]?.id ?? "") : null;
  const nextTicketId = activeIndex >= 0 && activeIndex < sortedFilteredItems.length - 1 ? String(sortedFilteredItems[activeIndex + 1]?.id ?? "") : null;

  function openTicket(id: string) {
    setListScrollTop(typeof window !== "undefined" ? window.scrollY : 0);
    setActiveId(id);
    if (typeof window !== "undefined") {
      window.scrollTo({ top: 0, behavior: "auto" });
    }
  }

  function backToList() {
    setActiveId(null);
    if (typeof window !== "undefined") {
      requestAnimationFrame(() => {
        window.scrollTo({ top: listScrollTop, behavior: "auto" });
      });
    }
  }

  return (
    <div className="grid gap-4">
      <div className="sticker-sm bg-white p-4">
        <div className="grid gap-3 md:grid-cols-2">
          <label className="grid gap-1 text-sm font-bold">
            <span className="text-xs uppercase tracking-wider text-slate-500">状态</span>
            <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-white px-3 py-2">
              <option value="all">全部</option>
              <option value="open">{ticketStatusLabel("open", locale)}</option>
              <option value="waiting_admin">{ticketStatusLabel("waiting_admin", locale)}</option>
              <option value="waiting_user">{ticketStatusLabel("waiting_user", locale)}</option>
              <option value="resolved">{ticketStatusLabel("resolved", locale)}</option>
              <option value="closed">{ticketStatusLabel("closed", locale)}</option>
            </select>
          </label>
          <label className="grid gap-1 text-sm font-bold">
            <span className="text-xs uppercase tracking-wider text-slate-500">优先级</span>
            <select value={priorityFilter} onChange={(e) => setPriorityFilter(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-white px-3 py-2">
              <option value="all">全部</option>
              <option value="low">{ticketPriorityOptionLabel("low", locale)}</option>
              <option value="normal">{ticketPriorityOptionLabel("normal", locale)}</option>
              <option value="high">{ticketPriorityOptionLabel("high", locale)}</option>
              <option value="urgent">{ticketPriorityOptionLabel("urgent", locale)}</option>
            </select>
          </label>
          <label className="grid gap-1 text-sm font-bold">
            <span className="text-xs uppercase tracking-wider text-slate-500">类型</span>
            <select value={typeFilter} onChange={(e) => setTypeFilter(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-white px-3 py-2">
              <option value="all">全部</option>
              <option value="feedback">{ticketTypeLabel("feedback", locale)}</option>
              <option value="billing_issue">{ticketTypeLabel("billing_issue", locale)}</option>
              <option value="account_issue">{ticketTypeLabel("account_issue", locale)}</option>
              <option value="payout_manual">{ticketTypeLabel("payout_manual", locale)}</option>
            </select>
          </label>
          <label className="grid gap-1 text-sm font-bold">
            <span className="text-xs uppercase tracking-wider text-slate-500">等待谁</span>
            <select value={waitingFilter} onChange={(e) => setWaitingFilter(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-white px-3 py-2">
              <option value="all">全部</option>
              <option value="admin">管理员</option>
              <option value="user">用户</option>
            </select>
          </label>
          <label className="grid gap-1 text-sm font-bold md:col-span-2">
            <span className="text-xs uppercase tracking-wider text-slate-500">关键词</span>
            <input value={keyword} onChange={(e) => setKeyword(e.target.value)} placeholder="主题 / 工单号 / user id" className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 outline-none focus:bg-white" />
          </label>
        </div>
      </div>

      {!activeId ? (
        <div className="grid gap-3">
          {items === null && <Skeleton className="h-20 w-full rounded-2xl" />}
          {items && sortedFilteredItems.length === 0 && <EmptyState shape="circle" title="没有匹配工单" hint="试试修改筛选条件" />}
          {items && sortedFilteredItems.map((t) => {
            const status = String(t.status ?? "open");
            const priority = String(t.priority ?? "normal");
            const waitingWho = waitingForLabel(status, locale);
            const autoCloseAt = t.auto_close_at ? String(t.auto_close_at) : "";
            const autoCloseRelative = autoCloseAt ? relativeCloseText(autoCloseAt) : "";
            return (
              <button key={String(t.id)} onClick={() => openTicket(String(t.id ?? ""))} className="text-left sticker bg-white p-4 lift">
                <div className="flex flex-wrap items-center justify-between gap-3">
                  <div className="flex flex-wrap items-center gap-2">
                    <span className="rounded-full border-2 border-slate-800 bg-amber-100 px-2 py-0.5 text-xs font-bold uppercase">{ticketTypeLabel(String(t.ticket_type ?? ""), locale)}</span>
                    <Pill status={status} size="sm">{ticketStatusLabel(status, locale)}</Pill>
                    <Pill variant={priorityVariantForAdmin(priority)} size="sm">{adminPriorityLabel(priority, locale)}</Pill>
                  </div>
                  <div className="text-xs text-slate-500">{String(t.ticket_no ?? "")} · {formatDate(String(t.created_at ?? ""))}</div>
                </div>
                <div className="mt-2 font-display text-base font-extrabold">{String(t.subject ?? "")}</div>
                <div className="mt-2 flex flex-wrap items-center gap-3 text-xs text-slate-500">
                  <span>当前：{waitingWho}</span>
                  {autoCloseAt && <span>{autoCloseRelative ? `自动关闭：${autoCloseRelative}` : `自动关闭：${formatDate(autoCloseAt)}`}</span>}
                  {Boolean(t.creator_user_id) && <span className="font-mono">user: {String(t.creator_user_id)}</span>}
                </div>
              </button>
            );
          })}
        </div>
      ) : (
        <div className="sticker bg-white p-6 min-h-[300px]">
          <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
            <div>
              <div className="text-sm text-slate-500">{String(activeTicket?.ticket_no ?? "")}</div>
              <div className="font-display text-2xl font-extrabold">工单详情</div>
            </div>
            <div className="flex flex-wrap gap-2">
              <button onClick={backToList} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">
                <ArrowLeft size={16} /> 返回工单列表
              </button>
              <button onClick={() => prevTicketId && setActiveId(prevTicketId)} disabled={!prevTicketId} className="rounded-full border-2 border-slate-800 bg-white px-4 py-2 text-sm font-bold lift disabled:opacity-40">
                上一条
              </button>
              <button onClick={() => nextTicketId && setActiveId(nextTicketId)} disabled={!nextTicketId} className="rounded-full border-2 border-slate-800 bg-white px-4 py-2 text-sm font-bold lift disabled:opacity-40">
                下一条
              </button>
            </div>
          </div>
          <AdminTicketDetails id={activeId} onChanged={reload} />
        </div>
      )}
    </div>
  );
}

function AdminTicketDetails({ id, onChanged }: { id: string; onChanged: () => void }) {
  const { locale } = useLocale();
  const toast = useToast();
  const [details, setDetails] = useState<AnyRow | null>(null);
  const [completeOpen, setCompleteOpen] = useState(false);
  const [reply, setReply] = useState("");
  const [internal, setInternal] = useState(false);
  const [replyFiles, setReplyFiles] = useState<File[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [statusOpen, setStatusOpen] = useState(false);
  const [statusValue, setStatusValue] = useState("waiting_user");
  const formatDate = useDateTimeFormatter();

  function reload() { apiGet<AnyRow>(`/v1/admin/tickets/${id}`).then(setDetails).catch(() => setDetails(null)); }
  useEffect(() => { reload(); }, [id]);
  if (!details) return <Skeleton className="h-40 w-full rounded-2xl" />;
  const ticket = (details.ticket ?? {}) as AnyRow;
  const messages = (Array.isArray(details.messages) ? details.messages : []) as AnyRow[];
  const attachments = (Array.isArray(details.attachments) ? details.attachments : []) as AnyRow[];
  const userMeta = (details.user_meta ?? null) as AnyRow | null;
  const userInfo = (userMeta?.user ?? null) as AnyRow | null;
  const sessionInfo = (userMeta?.session ?? null) as AnyRow | null;
  const isManualPayout = String(ticket.ticket_type ?? "") === "payout_manual";
  const autoCloseAt = details.auto_close_at ? String(details.auto_close_at) : null;
  const isClosed = String(ticket.status ?? "") === "closed" || String(ticket.status ?? "") === "resolved";

  async function send() {
    if (!reply.trim()) {
      toast.push({ variant: "error", title: "请输入回复内容" });
      return;
    }
    setSubmitting(true);
    try {
      const attachmentIds: string[] = [];
      for (const file of replyFiles) {
        const presigned = await apiPost<{ attachment_id: string; upload_url: string }>("/v1/ticket-attachments/presign", {
          filename: file.name,
          content_type: file.type || "application/octet-stream",
          byte_size: file.size
        });
        await apiPutBytes(presigned.upload_url, file);
        attachmentIds.push(presigned.attachment_id);
      }
      await apiPost(`/v1/admin/tickets/${id}/messages`, {
        body_text: reply.trim(),
        internal_note: internal,
        attachment_ids: attachmentIds
      });
      setReply("");
      setInternal(false);
      setReplyFiles([]);
      reload();
      onChanged();
      toast.push({ variant: "success", title: internal ? "已记录内部备注" : "已发送回复" });
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  async function applyStatus() {
    try {
      await apiPost(`/v1/admin/tickets/${id}/status`, { status: statusValue });
      toast.push({ variant: "success", title: "状态已更新" });
      reload();
      onChanged();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setStatusOpen(false);
    }
  }

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-xs uppercase tracking-wider text-slate-500">{ticketTypeLabel(String(ticket.ticket_type ?? ""), locale)}</div>
          <h3 className="font-display text-2xl font-extrabold">{String(ticket.subject ?? "")}</h3>
          <div className="text-xs text-slate-500">{String(ticket.ticket_no ?? "")}</div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Pill status={String(ticket.status ?? "open")}>{ticketStatusLabel(String(ticket.status ?? "open"), locale)}</Pill>
          <Pill variant="info">{adminPriorityLabel(String(ticket.priority ?? "normal"), locale)}</Pill>
          <button onClick={() => { setStatusValue(String(ticket.status ?? "open")); setStatusOpen(true); }} className="rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift">改状态</button>
          {isManualPayout && (
            <button onClick={() => setCompleteOpen(true)} className="rounded-full border-2 border-slate-800 bg-emerald-300 px-3 py-1 text-xs font-bold lift">完成人工提现</button>
          )}
        </div>
      </div>

      <div className="grid gap-3 sm:grid-cols-[1.4fr_1fr]">
        <div className="rounded-2xl border-2 border-slate-800 bg-white p-3 text-sm">
          <div className="text-xs font-bold uppercase tracking-wider text-slate-500">用户信息</div>
          <div className="mt-2 grid gap-1">
            <div><span className="text-slate-500">邮箱：</span><span className="font-mono">{String(userInfo?.email ?? "—")}</span></div>
            <div><span className="text-slate-500">用户 ID：</span><span className="font-mono break-all">{String(userInfo?.id ?? "—")}</span></div>
            <div><span className="text-slate-500">最近登录：</span>{formatDate(String(userInfo?.last_login_at ?? ""))}</div>
            <div><span className="text-slate-500">最近会话 IP：</span><span className="font-mono">{String(sessionInfo?.last_seen_ip ?? "—")}</span></div>
            <div><span className="text-slate-500">最近会话时间：</span>{formatDate(String(sessionInfo?.last_seen_at ?? ""))}</div>
            <div><span className="text-slate-500">最近会话地区：</span>{String(sessionInfo?.ip_country ?? "—")}</div>
          </div>
        </div>
        <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 p-3 text-sm">
          <div className="text-xs font-bold uppercase tracking-wider text-slate-500">状态说明</div>
          <div className="mt-2 text-slate-700">
            当前等待：<b>{waitingTargetLabel(String(ticket.status ?? ""), locale)}</b>
          </div>
          <div className="mt-1 text-slate-700">
            最近对外动作：<b>{String(details.last_external_sender ?? ticket.last_external_sender ?? "—")}</b>
          </div>
          {autoCloseAt ? (
            <div className="mt-2">若用户在 <span className="font-mono">{formatDate(autoCloseAt)}</span> 前未回复，将自动关闭</div>
          ) : (
            <div className="mt-2 text-slate-600">仅当工单为 waiting_user 且最近一条对外消息来自管理员时，才会进入 7 天倒计时</div>
          )}
        </div>
      </div>

      <div className="relative ml-4 grid gap-3 border-l-2 border-dashed border-slate-300 pl-5">
        {messages.map((m, i) => {
          const messageAttachments = attachments
            .filter((attachment) => String(attachment.message_id ?? "") === String(m.id ?? ""))
            .map((a, index) => ({
              id: String(a.id ?? index),
              original_filename: String(a.original_filename ?? a.object_key ?? "附件"),
              download_url: String(a.download_url ?? "#"),
              object_key: String(a.object_key ?? "")
            }));
          return (
            <div key={String(m.id ?? i)} className="relative">
              <span className="absolute -left-7 top-2 h-3 w-3 rounded-full border-2 border-slate-800 bg-violet-300" />
              <div className={`rounded-2xl border-2 border-slate-800 p-3 ${m.sender_type === "admin" ? "bg-amber-50" : m.sender_type === "system" ? "bg-slate-50" : "bg-emerald-50"}`}>
                <div className="flex items-center gap-2 text-xs">
                  <b>{String(m.sender_type ?? "")}</b>
                  {Boolean(m.internal_note) && <span className="rounded border border-dashed border-slate-500 px-1 text-slate-500">内部备注</span>}
                  <span className="text-slate-500">{formatDate(String(m.created_at ?? ""))}</span>
                </div>
                <p className="mt-1 whitespace-pre-wrap text-sm">{String(m.body_text ?? "")}</p>
                <div className="mt-3">
                  <ImageAttachmentGrid attachments={messageAttachments} />
                </div>
              </div>
            </div>
          );
        })}
      </div>

      {!isClosed && (
        <div className="rounded-2xl border-2 border-slate-800 bg-white p-3">
          <textarea value={reply} onChange={(e) => setReply(e.target.value)} placeholder="回复用户或留下内部备注…" className="min-h-24 w-full rounded-xl bg-amber-50 p-2 outline-none focus:bg-white" />
          <div className="mt-2"><FilePicker files={replyFiles} onChange={setReplyFiles} /></div>
          <div className="mt-3 flex flex-wrap items-center justify-between gap-3">
            <label className="inline-flex items-center gap-2 text-sm font-bold">
              <input type="checkbox" checked={internal} onChange={(e) => setInternal(e.target.checked)} className="h-4 w-4" /> 仅内部备注
            </label>
            <button onClick={send} disabled={submitting} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop disabled:opacity-50">
              {submitting ? "发送中…" : (internal ? "记录内部备注" : "发送给用户")}
            </button>
          </div>
        </div>
      )}

      <Modal open={statusOpen} onClose={() => setStatusOpen(false)} title="修改工单状态">
        <select value={statusValue} onChange={(e) => setStatusValue(e.target.value)} className="w-full rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 font-bold">
          <option value="open">{ticketStatusLabel("open", locale)}</option>
          <option value="waiting_user">{ticketStatusLabel("waiting_user", locale)}</option>
          <option value="waiting_admin">{ticketStatusLabel("waiting_admin", locale)}</option>
          <option value="resolved">{ticketStatusLabel("resolved", locale)}</option>
          <option value="closed">{ticketStatusLabel("closed", locale)}</option>
        </select>
        <ModalActions>
          <button onClick={() => setStatusOpen(false)} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
          <button onClick={applyStatus} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">保存</button>
        </ModalActions>
      </Modal>

      <CompleteManualPayoutModal open={completeOpen} ticket={ticket} onClose={() => setCompleteOpen(false)} onDone={() => { reload(); onChanged(); toast.push({ variant: "success", title: "已完成人工提现" }); }} />
    </div>
  );
}

function ticketTypeLabel(type: string, locale: "zh" | "en") {
  switch (type) {
    case "feedback":
      return locale === "en" ? "Feedback" : "使用反馈";
    case "billing_issue":
      return locale === "en" ? "Billing" : "充值/账单";
    case "account_issue":
      return locale === "en" ? "Account" : "账号/密钥";
    case "payout_manual":
      return locale === "en" ? "Manual Payout" : "人工提现";
    default:
      return type || (locale === "en" ? "Unknown" : "未知类型");
  }
}

function ticketStatusLabel(status: string, locale: "zh" | "en") {
  switch (status) {
    case "open":
      return locale === "en" ? "Open" : "待接单";
    case "waiting_admin":
      return locale === "en" ? "Waiting Admin" : "等待管理员";
    case "waiting_user":
      return locale === "en" ? "Waiting User" : "等待用户";
    case "resolved":
      return locale === "en" ? "Resolved" : "已解决";
    case "closed":
      return locale === "en" ? "Closed" : "已关闭";
    default:
      return status || (locale === "en" ? "Unknown" : "未知状态");
  }
}

function ticketPriorityOptionLabel(priority: string, locale: "zh" | "en") {
  switch (priority) {
    case "low":
      return locale === "en" ? "Low" : "低";
    case "high":
      return locale === "en" ? "High" : "高";
    case "urgent":
      return locale === "en" ? "Urgent" : "紧急";
    default:
      return locale === "en" ? "Normal" : "中";
  }
}

function adminPriorityLabel(priority: string, locale: "zh" | "en") {
  const label = ticketPriorityOptionLabel(priority, locale);
  return locale === "en" ? `Priority · ${label}` : `优先级 · ${label}`;
}

function waitingTargetLabel(status: string, locale: "zh" | "en") {
  if (status === "waiting_user") return locale === "en" ? "User" : "用户";
  if (status === "waiting_admin" || status === "open") return locale === "en" ? "Admin" : "管理员";
  return locale === "en" ? "None" : "无";
}

function waitingForLabel(status: string, locale: "zh" | "en") {
  if (status === "waiting_user") return locale === "en" ? "Waiting User" : "等待用户";
  if (status === "waiting_admin") return locale === "en" ? "Waiting Admin" : "等待管理员";
  if (status === "open") return locale === "en" ? "Open" : "待接单";
  return ticketStatusLabel(status, locale);
}

function priorityVariantForAdmin(priority: string) {
  switch (priority) {
    case "low": return "neutral";
    case "high": return "warning";
    case "urgent": return "failed";
    default: return "info";
  }
}

function relativeCloseText(iso: string) {
  const target = new Date(iso).getTime();
  if (!Number.isFinite(target)) return "";
  const diffMs = target - Date.now();
  const abs = Math.abs(diffMs);
  const day = 24 * 60 * 60 * 1000;
  const hour = 60 * 60 * 1000;
  if (diffMs <= 0) {
    const hours = Math.floor(abs / hour);
    return hours < 1 ? "已超时" : `已超时 ${hours} 小时`;
  }
  const days = Math.floor(diffMs / day);
  if (days >= 1) return `${days} 天后`;
  const hours = Math.max(1, Math.floor(diffMs / hour));
  return `${hours} 小时后`;
}

function priorityRank(priority: string) {
  switch (priority) {
    case "urgent": return 0;
    case "high": return 1;
    case "normal": return 2;
    case "low": return 3;
    default: return 4;
  }
}

function statusRank(status: string) {
  switch (status) {
    case "waiting_admin": return 0;
    case "open": return 1;
    case "waiting_user": return 2;
    case "resolved": return 3;
    case "closed": return 4;
    default: return 5;
  }
}

function autoCloseUrgencyValue(autoCloseAt?: string) {
  if (!autoCloseAt) return Number.POSITIVE_INFINITY;
  const target = new Date(autoCloseAt).getTime();
  return Number.isFinite(target) ? target : Number.POSITIVE_INFINITY;
}

function compareTicketCards(a: AnyRow, b: AnyRow) {
  const statusDiff = statusRank(String(a.status ?? "")) - statusRank(String(b.status ?? ""));
  if (statusDiff !== 0) return statusDiff;
  const priorityDiff = priorityRank(String(a.priority ?? "normal")) - priorityRank(String(b.priority ?? "normal"));
  if (priorityDiff !== 0) return priorityDiff;
  const autoDiff = autoCloseUrgencyValue(a.auto_close_at ? String(a.auto_close_at) : undefined) - autoCloseUrgencyValue(b.auto_close_at ? String(b.auto_close_at) : undefined);
  if (autoDiff !== 0) return autoDiff;
  const aUpdated = new Date(String(a.updated_at ?? a.created_at ?? 0)).getTime();
  const bUpdated = new Date(String(b.updated_at ?? b.created_at ?? 0)).getTime();
  return bUpdated - aUpdated;
}

function CompleteManualPayoutModal({ open, ticket, onClose, onDone }: { open: boolean; ticket: AnyRow; onClose: () => void; onDone: () => void }) {
  const toast = useToast();
  const [external, setExternal] = useState("");
  const [reason, setReason] = useState("");
  useEffect(() => { if (open) { setExternal(""); setReason(""); } }, [open]);
  async function submit() {
    if (!external.trim() || !reason.trim()) { toast.push({ variant: "error", title: "请填写 external tx id 与凭证" }); return; }
    try {
      await apiPost(`/v1/admin/tickets/${ticket.id}/complete-manual-payout`, {
        external_tx_id: external.trim(), reason: reason.trim(), proof: { note: reason.trim() }
      });
      onDone(); onClose();
    } catch (err) {
      toast.push({ variant: "error", title: "操作失败", description: String(err).replace(/^Error:\s*/, "") });
    }
  }
  return (
    <Modal open={open} onClose={onClose} title="完成人工提现" description={`工单 ${String(ticket.ticket_no ?? "")}`}>
      <input value={external} onChange={(e) => setExternal(e.target.value)} placeholder="external tx id" className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 font-mono outline-none focus:bg-white" />
      <textarea value={reason} onChange={(e) => setReason(e.target.value)} placeholder="凭证说明 / 备注" className="mt-3 min-h-24 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={submit} className="mt-5 rounded-full border-2 border-slate-800 bg-emerald-400 px-5 py-2 font-bold text-white btn-pop">确认完成</button>
      </ModalActions>
    </Modal>
  );
}

function AnnouncementsTab() {
  const { locale } = useLocale();
  const toast = useToast();
  const [items, setItems] = useState<SiteAnnouncement[]>(readAnnouncements());
  const [selectedId, setSelectedId] = useState(readAnnouncements()[0]?.id ?? "");
  const [editorOpen, setEditorOpen] = useState(false);
  const [editTarget, setEditTarget] = useState<SiteAnnouncement | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<SiteAnnouncement | null>(null);
  const formatDate = useDateTimeFormatter();

  useEffect(() => {
    function refresh() {
      const next = readAnnouncements();
      setItems(next);
      if (!next.find((item) => item.id === selectedId)) {
        setSelectedId(next[0]?.id ?? "");
      }
    }
    window.addEventListener("cc-switch-market:announcements-updated", refresh as EventListener);
    return () => window.removeEventListener("cc-switch-market:announcements-updated", refresh as EventListener);
  }, [selectedId]);

  const current = items.find((item) => item.id === selectedId) ?? items[0];

  async function copyCurrent() {
    if (!current) return;
    await navigator.clipboard.writeText(JSON.stringify(current, null, 2));
    toast.push({ variant: "success", title: "已复制公告 JSON" });
  }

  function save(next: SiteAnnouncement) {
    const exists = items.some((item) => item.id === next.id);
    const updated = exists ? items.map((item) => item.id === next.id ? next : item) : [next, ...items];
    writeAnnouncements(updated);
    setItems(updated);
    setSelectedId(next.id);
    toast.push({ variant: "success", title: exists ? "公告已更新" : "公告已新增" });
  }

  function remove(target: SiteAnnouncement) {
    const updated = items.filter((item) => item.id !== target.id);
    writeAnnouncements(updated);
    setItems(updated);
    setSelectedId(updated[0]?.id ?? "");
    toast.push({ variant: "success", title: "公告已删除" });
  }

  return (
    <div className="grid gap-4 lg:grid-cols-[0.9fr_1.1fr]">
      <div className="grid gap-3">
        <div className="flex justify-end">
          <button onClick={() => { setEditTarget(null); setEditorOpen(true); }} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop">
            <Plus size={16} /> 新增公告
          </button>
        </div>
        {items.length === 0 && <EmptyState shape="circle" title="暂无公告" />}
        {items.map((item) => (
          <button
            key={item.id}
            onClick={() => setSelectedId(item.id)}
            className={`text-left sticker-sm p-4 lift ${selectedId === item.id ? "bg-violet-100" : "bg-white"}`}
          >
            <div className="flex items-center justify-between gap-3">
              <span className="font-bold">{item.title[locale]}</span>
              <div className="flex items-center gap-2">
                {item.pinned && <Pill variant="warning">置顶</Pill>}
                <Pill variant={item.active === false ? "neutral" : "info"}>{item.active === false ? "停用" : "启用"}</Pill>
              </div>
            </div>
            <div className="mt-1 text-xs text-slate-500">{item.id} · {formatDate(item.publishedAt)}</div>
          </button>
        ))}
      </div>
      <div className="sticker bg-white p-6">
        {current ? (
          <div className="grid gap-4">
            <div className="flex items-center justify-between gap-3">
              <h3 className="font-display text-2xl font-extrabold">公告管理</h3>
              <div className="flex flex-wrap gap-2">
                <button onClick={copyCurrent} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-amber-100 px-3 py-1 text-sm font-bold lift">
                  <Copy size={14} /> 复制 JSON
                </button>
                <button onClick={() => { setEditTarget(current); setEditorOpen(true); }} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-sm font-bold lift">
                  <Pencil size={14} /> 编辑
                </button>
                <button onClick={() => setDeleteTarget(current)} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-pink-200 px-3 py-1 text-sm font-bold lift">
                  <Trash2 size={14} /> 删除
                </button>
              </div>
            </div>
            <p className="text-sm text-slate-500">
              当前公告事实源仍是本地浏览器存储。此入口可增删改并即时影响导航铃铛与公告弹窗
            </p>
            <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 p-4">
              <div className="text-xs uppercase tracking-wider text-slate-500">中文标题</div>
              <div className="mt-1 font-bold">{current.title.zh}</div>
              <div className="mt-3 text-xs uppercase tracking-wider text-slate-500">英文标题</div>
              <div className="mt-1 font-bold">{current.title.en}</div>
              <div className="mt-3 text-xs uppercase tracking-wider text-slate-500">中文正文</div>
              <div className="mt-1 text-sm leading-6">{current.body.zh}</div>
              <div className="mt-3 text-xs uppercase tracking-wider text-slate-500">英文正文</div>
              <div className="mt-1 text-sm leading-6">{current.body.en}</div>
            </div>
            <pre className="overflow-auto rounded-2xl border-2 border-slate-800 bg-slate-900 p-4 text-xs text-emerald-200">{JSON.stringify(current, null, 2)}</pre>
          </div>
        ) : (
          <EmptyState shape="circle" title="暂无公告" />
        )}
      </div>
      <AnnouncementEditorModal open={editorOpen} target={editTarget} onClose={() => setEditorOpen(false)} onSave={save} />
      <DeleteAnnouncementModal target={deleteTarget} onClose={() => setDeleteTarget(null)} onDelete={remove} />
    </div>
  );
}

function AnnouncementEditorModal({ open, target, onClose, onSave }: { open: boolean; target: SiteAnnouncement | null; onClose: () => void; onSave: (item: SiteAnnouncement) => void }) {
  const [form, setForm] = useState<SiteAnnouncement>({
    id: "",
    type: "system",
    title: { zh: "", en: "" },
    body: { zh: "", en: "" },
    publishedAt: new Date().toISOString(),
    pinned: false,
    active: true
  });

  useEffect(() => {
    if (target) setForm(target);
    else if (open) {
      const now = new Date().toISOString();
      setForm({
        id: `announcement-${Date.now()}`,
        type: "system",
        title: { zh: "", en: "" },
        body: { zh: "", en: "" },
        publishedAt: now,
        pinned: false,
        active: true
      });
    }
  }, [open, target]);

  return (
    <Modal open={open} onClose={onClose} title={target ? "编辑公告" : "新增公告"} width="lg">
      <div className="grid gap-3 sm:grid-cols-2">
        <Field label="ID"><input value={form.id} onChange={(e) => setForm({ ...form, id: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <Field label="类型">
          <select value={form.type} onChange={(e) => setForm({ ...form, type: e.target.value as SiteAnnouncement['type'] })} className="w-full rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 font-bold">
            <option value="system">system</option>
            <option value="billing">billing</option>
            <option value="pricing">pricing</option>
            <option value="maintenance">maintenance</option>
          </select>
        </Field>
        <Field label="中文标题"><input value={form.title.zh} onChange={(e) => setForm({ ...form, title: { ...form.title, zh: e.target.value } })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 outline-none focus:bg-white" /></Field>
        <Field label="英文标题"><input value={form.title.en} onChange={(e) => setForm({ ...form, title: { ...form.title, en: e.target.value } })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 outline-none focus:bg-white" /></Field>
        <Field label="发布时间"><input value={form.publishedAt} onChange={(e) => setForm({ ...form, publishedAt: e.target.value })} className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 font-mono outline-none focus:bg-white" /></Field>
        <label className="flex items-center gap-3 text-sm font-bold"><input type="checkbox" checked={!!form.pinned} onChange={(e) => setForm({ ...form, pinned: e.target.checked })} /> 置顶</label>
        <label className="flex items-center gap-3 text-sm font-bold"><input type="checkbox" checked={form.active !== false} onChange={(e) => setForm({ ...form, active: e.target.checked })} /> 启用</label>
        <Field label="中文正文" full><textarea value={form.body.zh} onChange={(e) => setForm({ ...form, body: { ...form.body, zh: e.target.value } })} className="min-h-28 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 outline-none focus:bg-white" /></Field>
        <Field label="英文正文" full><textarea value={form.body.en} onChange={(e) => setForm({ ...form, body: { ...form.body, en: e.target.value } })} className="min-h-28 w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-3 py-2 outline-none focus:bg-white" /></Field>
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={() => { onSave(form); onClose(); }} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">保存</button>
      </ModalActions>
    </Modal>
  );
}

function DeleteAnnouncementModal({ target, onClose, onDelete }: { target: SiteAnnouncement | null; onClose: () => void; onDelete: (item: SiteAnnouncement) => void }) {
  return (
    <Modal open={!!target} onClose={onClose} title="删除公告">
      <p className="text-sm text-slate-600">确认删除公告：{target?.title.zh}</p>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">取消</button>
        <button onClick={() => { if (target) onDelete(target); onClose(); }} className="mt-5 rounded-full border-2 border-slate-800 bg-pink-400 px-5 py-2 font-bold text-white btn-pop">确认删除</button>
      </ModalActions>
    </Modal>
  );
}

function LedgerCheckTab() {
  const [check, setCheck] = useState<{ ok?: boolean; details?: unknown } | null>(null);
  function reload() {
    apiGet<{ ok?: boolean; details?: unknown }>("/v1/admin/ledger/check").then(setCheck).catch(() => setCheck({ ok: false }));
  }
  useEffect(() => { reload(); }, []);
  return (
    <div className="grid gap-4">
      <div className={`sticker p-5 ${check?.ok ? "bg-emerald-100" : "bg-pink-100"}`}>
        <div className="flex flex-wrap items-center justify-between gap-3">
          <div>
            <h3 className="font-display text-xl font-extrabold">账本一致性</h3>
            <p className="text-sm text-slate-700">余额与账本聚合应完全一致</p>
          </div>
          <div className="flex items-center gap-3">
            <Pill variant={check?.ok ? "success" : "failed"}>{check?.ok ? "OK" : "异常"}</Pill>
            <button onClick={reload} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-xs font-bold lift"><RefreshCw size={14} /> 复核</button>
          </div>
        </div>
        {check?.details ? (
          <pre className="mt-3 overflow-auto rounded-xl border-2 border-slate-800 bg-slate-900 p-3 text-xs text-emerald-200">{JSON.stringify(check.details, null, 2)}</pre>
        ) : null}
      </div>
    </div>
  );
}

function SettingsTab() {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].admin.settings;
  const formatDate = useDateTimeFormatter();
  const [settings, setSettings] = useState<AdminSettings | null>(null);
  const [active, setActive] = useState("system");
  const [envValues, setEnvValues] = useState<Record<string, string>>({});
  const [initialEnv, setInitialEnv] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);
  const [previewOpen, setPreviewOpen] = useState(false);
  const [revealed, setRevealed] = useState<Record<string, boolean>>({});
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [footerItems, setFooterItems] = useState<FooterLinkItem[]>([]);
  const [initialFooter, setInitialFooter] = useState<FooterLinkItem[]>([]);
  const [footerErrors, setFooterErrors] = useState<Record<number, string>>({});

  useEffect(() => {
    apiGet<AdminSettings>("/v1/admin/settings")
      .then((value) => {
        setSettings(value);
        const next = envValuesFromSettings(value);
        setEnvValues(next);
        setInitialEnv(next);
        const links = value.footerLinks ?? [];
        setFooterItems(links.map(cloneFooterLink));
        setInitialFooter(links.map(cloneFooterLink));
      })
      .catch(() => setSettings({ timeZoneOffsetMinutes: 480, adminTablePageSize: 20, env: emptyEnvSettings(), footerLinks: [], footerIcons: [] }));
  }, []);

  const offset = settings?.timeZoneOffsetMinutes ?? 480;
  async function saveTz(nextOffset: number) {
    setSaving(true);
    try {
      const next = await apiPutJson<{ timeZoneOffsetMinutes: number }>("/v1/admin/settings", {
        timeZoneOffsetMinutes: nextOffset,
      });
      setSettings((current) => ({
        ...(current ?? { env: emptyEnvSettings() }),
        timeZoneOffsetMinutes: next.timeZoneOffsetMinutes,
      }));
      updateCachedPublicConfig({ timeZoneOffsetMinutes: next.timeZoneOffsetMinutes });
      toast.push({ variant: "success", title: c.system.savedToast });
    } catch (err) {
      toast.push({ variant: "error", title: c.system.saveFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSaving(false);
    }
  }
  async function saveAdminTablePageSize(nextPageSize: number) {
    setSaving(true);
    try {
      const next = await apiPutJson<{ adminTablePageSize: number }>("/v1/admin/settings", {
        adminTablePageSize: nextPageSize,
      });
      setSettings((current) => ({
        ...(current ?? { timeZoneOffsetMinutes: offset, env: emptyEnvSettings() }),
        adminTablePageSize: next.adminTablePageSize,
      }));
      updateCachedPublicConfig({ adminTablePageSize: next.adminTablePageSize });
      toast.push({ variant: "success", title: c.system.tablePageSizeSavedToast });
    } catch (err) {
      toast.push({ variant: "error", title: c.system.saveFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSaving(false);
    }
  }
  async function saveEnv() {
    setSaving(true);
    try {
      const next = await apiPutJson<{ env: AdminEnvSettings }>("/v1/admin/settings/env", {
        values: envValues,
      });
      setSettings((current) => ({
        ...(current ?? { timeZoneOffsetMinutes: offset }),
        env: next.env,
      }));
      const refreshed = envValuesFromEnv(next.env);
      setEnvValues(refreshed);
      setInitialEnv(refreshed);
      setPreviewOpen(false);
      toast.push({ variant: "success", title: c.env.savedToast, description: c.env.savedDesc });
    } catch (err) {
      toast.push({ variant: "error", title: c.env.saveFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSaving(false);
    }
  }

  const env = settings?.env ?? emptyEnvSettings();
  const adminTablePageSize = settings?.adminTablePageSize ?? 20;
  const tabs: { key: string; label: string }[] = [
    { key: "system", label: c.tabs.system },
    { key: "version", label: c.tabs.version },
    ...env.categories.map((cat) => ({ key: cat.key, label: c.categories[cat.key as keyof typeof c.categories] ?? cat.key })),
  ];
  const visibleFields = env.fields.filter((field) => field.category === active);
  const dirtyKeys = env.fields
    .map((field) => field.key)
    .filter((key) => (envValues[key] ?? "") !== (initialEnv[key] ?? ""));
  const isDirty = dirtyKeys.length > 0;

  function dirtyCountForCategory(catKey: string): number {
    return env.fields.filter((field) => field.category === catKey && (envValues[field.key] ?? "") !== (initialEnv[field.key] ?? "")).length;
  }
  function missingCountForCategory(catKey: string): number {
    return env.fields.filter((field) => field.category === catKey && field.required && !(envValues[field.key] ?? "").trim()).length;
  }

  function resetField(key: string) {
    setEnvValues((current) => ({ ...current, [key]: initialEnv[key] ?? "" }));
  }
  function restoreDefault(field: AdminEnvField) {
    setEnvValues((current) => ({ ...current, [field.key]: field.defaultValue ?? "" }));
  }
  function resetAll() {
    setEnvValues({ ...initialEnv });
  }
  function toggleReveal(key: string) {
    setRevealed((current) => ({ ...current, [key]: !current[key] }));
  }
  async function copyValue(key: string, value: string) {
    if (typeof navigator === "undefined" || !navigator.clipboard) return;
    try {
      await navigator.clipboard.writeText(value);
      setCopiedKey(key);
      setTimeout(() => setCopiedKey((cur) => (cur === key ? null : cur)), 1400);
    } catch {
      // ignore clipboard failures
    }
  }

  const footerDirty = !footerLinkListsEqual(footerItems, initialFooter);
  const fl = c.footerLinks;
  const footerIconChoices = settings?.footerIcons ?? FALLBACK_FOOTER_ICONS;

  function validateFooterItems(items: FooterLinkItem[]): { ok: boolean; errors: Record<number, string> } {
    const errors: Record<number, string> = {};
    items.forEach((item, idx) => {
      if (!item.labelZh.trim() && !item.labelEn.trim()) {
        errors[idx] = fl.errorMissingLabel;
        return;
      }
      const url = item.url.trim();
      if (!url) {
        errors[idx] = fl.errorMissingUrl;
        return;
      }
      const ok = /^(https?:\/\/|\/|mailto:|#$)/.test(url);
      if (!ok) errors[idx] = fl.errorBadUrl;
    });
    return { ok: Object.keys(errors).length === 0, errors };
  }

  function updateFooterItem(idx: number, patch: Partial<FooterLinkItem>) {
    setFooterItems((cur) => cur.map((item, i) => (i === idx ? { ...item, ...patch } : item)));
  }
  function moveFooterItem(idx: number, delta: number) {
    setFooterItems((cur) => {
      const next = idx + delta;
      if (next < 0 || next >= cur.length) return cur;
      const copy = cur.slice();
      const [moved] = copy.splice(idx, 1);
      copy.splice(next, 0, moved);
      return copy;
    });
  }
  function removeFooterItem(idx: number) {
    setFooterItems((cur) => cur.filter((_, i) => i !== idx));
    setFooterErrors({});
  }
  function addFooterItem() {
    setFooterItems((cur) => [...cur, { labelZh: "", labelEn: "", url: "", icon: "link" }]);
  }
  function resetFooter() {
    setFooterItems(initialFooter.map(cloneFooterLink));
    setFooterErrors({});
  }

  async function saveFooter() {
    const { ok, errors } = validateFooterItems(footerItems);
    setFooterErrors(errors);
    if (!ok) return;
    setSaving(true);
    try {
      const next = await apiPutJson<{ footerLinks: FooterLinkItem[] }>("/v1/admin/settings/footer-links", {
        items: footerItems,
      });
      const refreshed = (next.footerLinks ?? []).map(cloneFooterLink);
      setFooterItems(refreshed.map(cloneFooterLink));
      setInitialFooter(refreshed);
      setSettings((current) => ({
        ...(current ?? { timeZoneOffsetMinutes: offset, env: emptyEnvSettings() }),
        footerLinks: refreshed,
      }));
      updateCachedPublicConfig({ footerLinks: refreshed });
      toast.push({ variant: "success", title: fl.savedToast, description: fl.savedDesc });
    } catch (err) {
      toast.push({ variant: "error", title: fl.saveFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSaving(false);
    }
  }

  useEffect(() => {
    const guardOn = isDirty || footerDirty;
    if (!guardOn) return;
    const handler = (event: BeforeUnloadEvent) => {
      event.preventDefault();
      event.returnValue = isDirty ? c.env.beforeUnload : fl.beforeUnload;
    };
    window.addEventListener("beforeunload", handler);
    return () => window.removeEventListener("beforeunload", handler);
  }, [isDirty, footerDirty, c.env.beforeUnload, fl.beforeUnload]);

  return (
    <div className="grid gap-4 md:grid-cols-[220px_minmax(0,1fr)]">
      <aside className="relative md:sticky md:top-4 md:self-start">
        <span aria-hidden className="pointer-events-none absolute -left-2 -top-3 hidden h-6 w-6 rotate-[-12deg] rounded-full border-2 border-slate-800 bg-emerald-300 md:block" />
        <nav aria-label={c.title} className="flex md:flex-col gap-2 overflow-x-auto md:overflow-visible pb-1">
          {tabs.map((tab) => {
            const isActive = active === tab.key;
            const dirtyCount = !["system", "version"].includes(tab.key) ? dirtyCountForCategory(tab.key) : 0;
            const missingCount = !["system", "version"].includes(tab.key) ? missingCountForCategory(tab.key) : 0;
            return (
              <button
                key={tab.key}
                onClick={() => setActive(tab.key)}
                aria-current={isActive ? "page" : undefined}
                className={`relative flex shrink-0 items-center justify-between gap-2 rounded-2xl border-2 border-slate-800 px-4 py-2 text-left text-sm font-bold transition-transform duration-200 motion-safe:hover:-translate-y-0.5 md:rounded-tl-2xl md:rounded-tr-2xl md:rounded-br-2xl md:rounded-bl-none ${isActive ? "bg-violet-500 text-white" : "bg-white text-slate-800"}`}
              >
                <span>{tab.label}</span>
                <span className="flex items-center gap-1">
                  {dirtyCount > 0 && (
                    <span aria-label={c.env.unsavedSummary(dirtyCount)} className="rounded-full border-2 border-slate-800 bg-amber-300 px-1.5 text-[10px] font-extrabold text-slate-800">{dirtyCount}</span>
                  )}
                  {missingCount > 0 && (
                    <span aria-label={c.env.required} title={c.env.required} className="rounded-full border-2 border-slate-800 bg-pink-400 px-1.5 text-[10px] font-extrabold text-white">!</span>
                  )}
                </span>
              </button>
            );
          })}
        </nav>
      </aside>

      <section className="grid gap-4">
        {active === "system" ? (
          <div className="sticker bg-white p-6">
            <div className="mb-4">
              <div className="font-display text-xl font-extrabold">{c.tabs.system}</div>
              <p className="mt-1 text-sm text-slate-500">{c.subtitle}</p>
            </div>
            <div className="grid gap-4 md:grid-cols-[1fr_1.3fr] md:items-end">
              <Field label={c.system.timeZoneLabel}>
                <select
                  value={offset}
                  onChange={(event) => saveTz(Number(event.target.value))}
                  disabled={!settings || saving}
                  className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 font-bold outline-none focus:bg-white disabled:opacity-60"
                >
                  {timeZoneOptions(c.system.timeZoneDefaultSuffix).map((option) => (
                    <option key={option.value} value={option.value}>{option.label}</option>
                  ))}
                </select>
              </Field>
              <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 p-4 text-sm">
                <div className="font-bold">{c.system.timeZoneCurrent}：{formatUtcOffset(offset)}</div>
                <div className="mt-1 font-mono text-slate-600">{formatDate(new Date())}</div>
              </div>
              <Field label={c.system.tablePageSizeLabel}>
                <input
                  type="number"
                  min="1"
                  max="500"
                  value={adminTablePageSize}
                  onChange={(event) => {
                    const next = Number(event.target.value);
                    if (Number.isFinite(next)) {
                      setSettings((current) => ({
                        ...(current ?? { timeZoneOffsetMinutes: offset, env: emptyEnvSettings() }),
                        adminTablePageSize: next,
                      }));
                    }
                  }}
                  onBlur={(event) => saveAdminTablePageSize(Math.min(500, Math.max(1, Math.floor(Number(event.target.value) || 20))))}
                  disabled={!settings || saving}
                  className="w-full rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 font-mono font-bold outline-none focus:bg-white disabled:opacity-60"
                />
              </Field>
              <div className="rounded-2xl border-2 border-slate-800 bg-violet-50 p-4 text-sm text-slate-700">
                <div className="font-bold">{c.system.tablePageSizeCurrent(adminTablePageSize)}</div>
                <div className="mt-1 text-xs text-slate-500">{c.system.tablePageSizeHint}</div>
              </div>
            </div>
            <FooterLinksEditor
              items={footerItems}
              iconChoices={footerIconChoices}
              errors={footerErrors}
              dirty={footerDirty}
              saving={saving}
              text={fl}
              onAdd={addFooterItem}
              onReset={resetFooter}
              onSave={saveFooter}
              onChangeItem={updateFooterItem}
              onMove={moveFooterItem}
              onRemove={removeFooterItem}
            />
          </div>
        ) : active === "version" ? (
          <VersionPanel />
        ) : (
          <div className="sticker bg-white p-6">
            <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
              <div className="min-w-0">
                <div className="font-display text-xl font-extrabold">{c.categories[active as keyof typeof c.categories] ?? active}</div>
                <div className="mt-1 text-xs text-slate-500">
                  {c.env.envFileLabel}：<span className="font-mono break-all">{env.envFile || "—"}</span>
                </div>
                <div className="mt-1 text-xs font-bold">
                  {isDirty ? <span className="text-amber-700">{c.env.unsavedSummary(dirtyKeys.length)}</span> : <span className="text-emerald-700">{c.env.noUnsaved}</span>}
                </div>
              </div>
              <div className="flex flex-wrap items-center gap-2">
                <button onClick={resetAll} disabled={!isDirty || saving} className="rounded-full border-2 border-slate-800 bg-white px-4 py-2 text-sm font-bold disabled:opacity-50 hover:bg-amber-300">
                  {c.env.reset}
                </button>
                <button onClick={() => setPreviewOpen(true)} disabled={!isDirty || saving} className="rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white disabled:opacity-50">
                  {c.env.save}
                </button>
              </div>
            </div>
            <div className="mb-4 rounded-2xl border-2 border-slate-800 bg-amber-50 p-3 text-sm text-slate-700">
              {c.env.hint}
            </div>
            <div className="grid gap-4">
              {visibleFields.map((field) => (
                <EnvFieldRow
                  key={field.key}
                  field={field}
                  locale={locale}
                  value={envValues[field.key] ?? ""}
                  initialValue={initialEnv[field.key] ?? ""}
                  revealed={!!revealed[field.key]}
                  copied={copiedKey === field.key}
                  onChange={(next) => setEnvValues((cur) => ({ ...cur, [field.key]: next }))}
                  onToggleReveal={() => toggleReveal(field.key)}
                  onCopy={() => copyValue(field.key, envValues[field.key] ?? "")}
                  onResetField={() => resetField(field.key)}
                  onRestoreDefault={() => restoreDefault(field)}
                />
              ))}
            </div>
          </div>
        )}
      </section>

      <Modal open={previewOpen} onClose={() => setPreviewOpen(false)} title={c.env.previewTitle}>
        <p className="text-sm text-slate-600">{c.env.previewBody(dirtyKeys.length)}</p>
        <ul className="mt-3 max-h-80 overflow-auto rounded-2xl border-2 border-slate-800 bg-amber-50 p-3 text-sm">
          {dirtyKeys.map((key) => {
            const field = env.fields.find((f) => f.key === key);
            const before = initialEnv[key] ?? "";
            const after = envValues[key] ?? "";
            const isSecret = !!field?.secret;
            const mask = (v: string) => (isSecret && v ? "••••••" : v || c.env.empty);
            return (
              <li key={key} className="grid gap-1 border-b border-dashed border-slate-300 py-2 last:border-0">
                <div className="font-mono text-xs text-slate-500">{key}</div>
                <div className="font-mono text-xs">
                  <span className="text-rose-700 line-through">{mask(before)}</span>
                  <span className="mx-2 text-slate-400">→</span>
                  <span className="text-emerald-700">{mask(after)}</span>
                </div>
              </li>
            );
          })}
        </ul>
        <ModalActions>
          <button onClick={() => setPreviewOpen(false)} disabled={saving} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold disabled:opacity-50">
            {c.env.previewCancel}
          </button>
          <button onClick={saveEnv} disabled={saving} className="rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white disabled:opacity-50">
            {c.env.previewConfirm}
          </button>
        </ModalActions>
      </Modal>
    </div>
  );
}

type AdminSettings = {
  timeZoneOffsetMinutes: number;
  adminTablePageSize?: number;
  env: AdminEnvSettings;
  footerLinks?: FooterLinkItem[];
  footerIcons?: string[];
};

type AdminVersionInfo = {
  version: string;
  git_sha: string;
  git_ref: string;
  build_time: string;
  target: string;
  pid: number;
  uptime_seconds: number;
  current_exe: string;
  binary_path: string;
  log_path: string;
  service_name: string;
  service_exists: boolean;
  release_binary_url: string;
};

function VersionPanel() {
  const { locale } = useLocale();
  const c = copy[locale].admin.settings.version;
  const toast = useToast();
  const [info, setInfo] = useState<AdminVersionInfo | null>(null);
  const [loading, setLoading] = useState(true);
  const [busy, setBusy] = useState<"restart" | "update" | null>(null);

  function reload() {
    setLoading(true);
    apiGet<AdminVersionInfo>("/v1/admin/version")
      .then(setInfo)
      .catch(() => setInfo(null))
      .finally(() => setLoading(false));
  }

  useEffect(() => { reload(); }, []);

  async function waitForService() {
    const deadline = Date.now() + 90_000;
    while (Date.now() < deadline) {
      await new Promise((resolve) => setTimeout(resolve, 2500));
      try {
        await apiGet<AdminVersionInfo>(`/v1/admin/version?ts=${Date.now()}`);
        window.location.reload();
        return;
      } catch {
        // service is still restarting
      }
    }
    window.location.reload();
  }

  async function runAction(kind: "restart" | "update") {
    if (!window.confirm(kind === "restart" ? c.confirmRestart : c.confirmUpdate)) return;
    setBusy(kind);
    try {
      await apiPost<{ ok: boolean; mode: string }>(`/v1/admin/version/${kind}`, {});
      toast.push({
        variant: "success",
        title: kind === "restart" ? c.restartScheduled : c.updateScheduled,
        description: c.recovering,
      });
      void waitForService();
    } catch (err) {
      toast.push({ variant: "error", title: c.actionFailed, description: String(err).replace(/^Error:\s*/, "") });
      setBusy(null);
    }
  }

  const rows = info ? [
    [c.currentVersion, info.version],
    [c.commit, shortSha(info.git_sha)],
    [c.gitRef, info.git_ref || c.unknown],
    [c.buildTime, info.build_time || c.unknown],
    [c.target, info.target],
    [c.pid, String(info.pid)],
    [c.uptime, formatUptime(info.uptime_seconds)],
    [c.serviceMode, info.service_exists ? c.systemd : c.manual],
    [c.currentExe, info.current_exe],
    [c.binaryPath, info.binary_path],
    [c.logPath, info.log_path],
    [c.releaseUrl, info.release_binary_url],
  ] : [];

  return (
    <div className="sticker bg-white p-6">
      <div className="mb-4 flex flex-wrap items-start justify-between gap-3">
        <div>
          <div className="font-display text-xl font-extrabold">{c.title}</div>
          <p className="mt-1 text-sm text-slate-500">{c.subtitle}</p>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <button onClick={() => window.location.reload()} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-4 py-2 text-sm font-bold hover:bg-amber-300">
            <RefreshCw size={15} /> {c.pageRefresh}
          </button>
          <button onClick={() => runAction("restart")} disabled={!!busy} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-amber-300 px-4 py-2 text-sm font-bold disabled:opacity-50">
            <RotateCw size={15} /> {busy === "restart" ? c.recovering : c.restart}
          </button>
          <button onClick={() => runAction("update")} disabled={!!busy} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 text-sm font-bold text-white disabled:opacity-50">
            <RefreshCw size={15} /> {busy === "update" ? c.recovering : c.update}
          </button>
        </div>
      </div>
      {loading ? (
        <div className="rounded-2xl border-2 border-slate-200 bg-amber-50 p-6 text-sm font-bold text-slate-600">{c.loading}</div>
      ) : !info ? (
        <div className="rounded-2xl border-2 border-pink-300 bg-pink-50 p-6 text-sm font-bold text-pink-700">{c.loadFailed}</div>
      ) : (
        <dl className="grid gap-3 md:grid-cols-2">
          {rows.map(([label, value]) => (
            <div key={label} className="rounded-2xl border-2 border-slate-200 bg-amber-50 p-4">
              <dt className="text-xs font-bold uppercase tracking-wider text-slate-500">{label}</dt>
              <dd className="mt-1 break-all font-mono text-sm font-bold text-slate-900">{value}</dd>
            </div>
          ))}
        </dl>
      )}
    </div>
  );
}

function shortSha(value: string): string {
  if (!value || value === "unknown") return value || "unknown";
  return value.slice(0, 12);
}

function formatUptime(seconds: number): string {
  const day = Math.floor(seconds / 86400);
  const hour = Math.floor((seconds % 86400) / 3600);
  const minute = Math.floor((seconds % 3600) / 60);
  const second = Math.floor(seconds % 60);
  return [
    day ? `${day}d` : "",
    hour ? `${hour}h` : "",
    minute ? `${minute}m` : "",
    `${second}s`,
  ].filter(Boolean).join(" ");
}

type FooterLinkItem = {
  labelZh: string;
  labelEn: string;
  url: string;
  icon: string;
};

type AdminEnvSettings = {
  envFile?: string;
  categories: { key: string; label?: string }[];
  fields: AdminEnvField[];
};

type AdminEnvField = {
  key: string;
  category: string;
  labelZh: string;
  labelEn: string;
  descriptionZh: string;
  descriptionEn: string;
  kind: string;
  secret?: boolean;
  required?: boolean;
  defaultValue?: string;
  placeholder?: string;
  unit?: string;
  value?: string;
};

function emptyEnvSettings(): AdminEnvSettings {
  return { categories: [], fields: [] };
}

function envValuesFromSettings(settings: AdminSettings): Record<string, string> {
  return envValuesFromEnv(settings.env);
}

function envValuesFromEnv(env: AdminEnvSettings): Record<string, string> {
  return Object.fromEntries(env.fields.map((field) => [field.key, String(field.value ?? "")]));
}

type EnvCopy = (typeof copy)["zh"]["admin"]["settings"]["env"] | (typeof copy)["en"]["admin"]["settings"]["env"];

function unitLabel(unit: string | undefined, t: EnvCopy): string {
  switch (unit) {
    case "secs": return t.unitSecs;
    case "days": return t.unitDays;
    case "bps": return t.unitBps;
    case "USD": return t.unitUsd;
    default: return unit ?? "";
  }
}

function EnvFieldRow({
  field,
  locale,
  value,
  initialValue,
  revealed,
  copied,
  onChange,
  onToggleReveal,
  onCopy,
  onResetField,
  onRestoreDefault,
}: {
  field: AdminEnvField;
  locale: "zh" | "en";
  value: string;
  initialValue: string;
  revealed: boolean;
  copied: boolean;
  onChange: (next: string) => void;
  onToggleReveal: () => void;
  onCopy: () => void;
  onResetField: () => void;
  onRestoreDefault: () => void;
}) {
  const t = copy[locale].admin.settings.env;
  const label = locale === "zh" ? field.labelZh : field.labelEn;
  const description = locale === "zh" ? field.descriptionZh : field.descriptionEn;
  const isDirty = value !== initialValue;
  const defaultValue = field.defaultValue ?? "";
  const isAtDefault = value === defaultValue;
  const isMissing = !!field.required && !value.trim();
  const descriptionId = `envdesc-${field.key}`;

  const inputBase = "w-full rounded-2xl border-2 px-4 py-3 font-mono text-sm outline-none transition-colors focus:bg-white focus:border-violet-500 focus:ring-2 focus:ring-violet-300";
  const inputBorder = isMissing ? "border-pink-500 bg-pink-50" : "border-slate-800 bg-amber-50";
  const inputClass = `${inputBase} ${inputBorder}`;

  function renderInput() {
    if (field.kind === "bool") {
      const on = value === "true";
      return (
        <label className="flex items-center justify-between gap-3 rounded-2xl border-2 border-slate-800 bg-white px-4 py-3">
          <span className="text-sm font-bold text-slate-800">{on ? t.boolOn : t.boolOff}</span>
          <Switch
            checked={on}
            aria-describedby={descriptionId}
            onCheckedChange={(checked) => onChange(checked ? "true" : "false")}
          />
        </label>
      );
    }
    if (field.kind === "select" && field.key === "OBJECT_STORE_BACKEND") {
      return (
        <select value={value || defaultValue} onChange={(event) => onChange(event.target.value)} aria-describedby={descriptionId} className={`${inputClass} font-bold`}>
          <option value="local">local</option>
          <option value="r2">r2</option>
        </select>
      );
    }
    if (field.key === "DODO_ALLOWED_PAYMENT_METHOD_TYPES") {
      return (
        <div className="grid gap-3">
          <input
            type="text"
            value={value}
            aria-required={field.required ? true : undefined}
            aria-describedby={descriptionId}
            placeholder={field.placeholder || ""}
            onChange={(event) => onChange(event.target.value)}
            className={inputClass}
          />
          <div className="flex flex-wrap gap-2">
            {dodoPaymentMethodOptions(locale).map((option) => (
              <button
                key={option.value}
                type="button"
                onClick={() => onChange(appendCsvValue(value, option.value))}
                title={option.value}
                className="grid rounded-2xl border-2 border-slate-800 bg-white px-3 py-2 text-left text-xs hover:bg-amber-300"
              >
                <span className="font-extrabold">{option.label}</span>
                <span className="font-mono text-[10px] text-slate-500">{option.meta}</span>
              </button>
            ))}
          </div>
        </div>
      );
    }
    const inputType = field.secret && !revealed ? "password" : field.kind === "number" ? "number" : "text";
    return (
      <div className="relative">
        <input
          type={inputType}
          value={value}
          aria-required={field.required ? true : undefined}
          aria-describedby={descriptionId}
          placeholder={field.placeholder || ""}
          onChange={(event) => onChange(event.target.value)}
          className={`${inputClass} ${field.unit ? "pr-16" : field.secret ? "pr-24" : "pr-12"}`}
        />
        {field.unit && !field.secret && (
          <span className="pointer-events-none absolute right-3 top-1/2 -translate-y-1/2 rounded-full border-2 border-slate-800 bg-white px-2 py-0.5 text-[10px] font-extrabold uppercase tracking-wide text-slate-700">
            {unitLabel(field.unit, t)}
          </span>
        )}
        {field.secret && (
          <button type="button" onClick={onToggleReveal} className="absolute right-2 top-1/2 -translate-y-1/2 rounded-full border-2 border-slate-800 bg-white px-3 py-1 text-[11px] font-bold hover:bg-amber-300">
            {revealed ? t.hide : t.reveal}
          </button>
        )}
      </div>
    );
  }

  return (
    <div className={`relative grid gap-2 rounded-2xl border-2 border-slate-200 bg-white p-4 transition-colors ${isDirty ? "border-l-[6px] border-l-violet-500" : ""}`}>
      <div className="flex flex-wrap items-center justify-between gap-2">
        <div className="min-w-0">
          <div className="flex flex-wrap items-center gap-2">
            <span className="font-display text-base font-extrabold text-slate-900">{label}</span>
            {field.required ? (
              <span className="rounded-full border-2 border-slate-800 bg-violet-500 px-2 py-0.5 text-[10px] font-extrabold uppercase tracking-wide text-white">{t.required}</span>
            ) : (
              <span className="rounded-full border-2 border-slate-300 bg-slate-100 px-2 py-0.5 text-[10px] font-extrabold uppercase tracking-wide text-slate-600">{t.optional}</span>
            )}
            {isDirty && (
              <span className="rounded-full border-2 border-slate-800 bg-amber-300 px-2 py-0.5 text-[10px] font-extrabold uppercase tracking-wide text-slate-800">{t.modified}</span>
            )}
          </div>
          <div className="mt-0.5 font-mono text-[11px] text-slate-500 break-all">{field.key}</div>
        </div>
        <div className="flex items-center gap-1">
          {!field.secret && value && (
            <button type="button" onClick={onCopy} className="rounded-full border-2 border-slate-800 bg-white px-2 py-1 text-[11px] font-bold hover:bg-amber-300">
              {copied ? t.copied : t.copy}
            </button>
          )}
          {isDirty && (
            <button type="button" onClick={onResetField} aria-label={t.resetField} title={t.resetField} className="rounded-full border-2 border-slate-800 bg-white px-2 py-1 text-[11px] font-bold hover:bg-amber-300">
              ↺
            </button>
          )}
        </div>
      </div>
      <p id={descriptionId} className="text-xs text-slate-600">{description}</p>
      {renderInput()}
      {defaultValue !== "" && (
        <div className="flex flex-wrap items-center gap-2 text-[11px] text-slate-500">
          <span className="rounded-full border-2 border-emerald-400 bg-emerald-50 px-2 py-0.5 font-extrabold uppercase tracking-wide text-emerald-700">{t.defaultLabel}</span>
          <code className="font-mono break-all">{field.secret ? "••••••" : defaultValue}</code>
          {!isAtDefault && (
            <button type="button" onClick={onRestoreDefault} className="rounded-full border-2 border-slate-300 bg-white px-2 py-0.5 font-bold hover:bg-emerald-100">
              {t.restoreDefault}
            </button>
          )}
        </div>
      )}
    </div>
  );
}

function dodoPaymentMethodOptions(locale: "zh" | "en") {
  if (locale === "zh") {
    return [
      { label: "信用卡", value: "credit", meta: "Card · Global" },
      { label: "借记卡", value: "debit", meta: "Card · Global" },
      { label: "Apple Pay", value: "apple_pay", meta: "Wallet · Global" },
      { label: "Google Pay", value: "google_pay", meta: "Wallet · Global" },
      { label: "Amazon Pay", value: "amazon_pay", meta: "Wallet · US/EU/JP" },
      { label: "Cash App", value: "cashapp", meta: "Wallet · US/UK" },
      { label: "Klarna", value: "klarna", meta: "BNPL · US/EU/UK/AU" },
      { label: "Afterpay / Clearpay", value: "afterpay_clearpay", meta: "BNPL · US/UK/AU/NZ/CA" },
      { label: "UPI Collect", value: "upi_collect", meta: "Bank Transfer · India" },
      { label: "UPI Intent", value: "upi_intent", meta: "Bank Transfer · India" },
      { label: "iDEAL", value: "ideal", meta: "Bank Transfer · Netherlands" },
      { label: "Bancontact", value: "bancontact_card", meta: "Card · Belgium" },
      { label: "EPS", value: "eps", meta: "Bank Transfer · Austria" },
      { label: "Multibanco", value: "multibanco", meta: "Bank Transfer · Portugal" },
      { label: "Revolut Pay", value: "revolut_pay", meta: "Wallet · EU/UK" },
      { label: "Billie", value: "billie", meta: "BNPL · DE/AT/SE" },
      { label: "稳定币", value: "crypto_currency", meta: "Crypto · Global" },
      { label: "Pix", value: "pix", meta: "Bank Transfer · Brazil" },
      { label: "微信支付", value: "we_chat_pay", meta: "Wallet · China" },
    ];
  }
  return [
    { label: "Credit Card", value: "credit", meta: "Card · Global" },
    { label: "Debit Card", value: "debit", meta: "Card · Global" },
    { label: "Apple Pay", value: "apple_pay", meta: "Wallet · Global" },
    { label: "Google Pay", value: "google_pay", meta: "Wallet · Global" },
    { label: "Amazon Pay", value: "amazon_pay", meta: "Wallet · US/EU/JP" },
    { label: "Cash App", value: "cashapp", meta: "Wallet · US/UK" },
    { label: "Klarna", value: "klarna", meta: "BNPL · US/EU/UK/AU" },
    { label: "Afterpay / Clearpay", value: "afterpay_clearpay", meta: "BNPL · US/UK/AU/NZ/CA" },
    { label: "UPI Collect", value: "upi_collect", meta: "Bank Transfer · India" },
    { label: "UPI Intent", value: "upi_intent", meta: "Bank Transfer · India" },
    { label: "iDEAL", value: "ideal", meta: "Bank Transfer · Netherlands" },
    { label: "Bancontact", value: "bancontact_card", meta: "Card · Belgium" },
    { label: "EPS", value: "eps", meta: "Bank Transfer · Austria" },
    { label: "Multibanco", value: "multibanco", meta: "Bank Transfer · Portugal" },
    { label: "Revolut Pay", value: "revolut_pay", meta: "Wallet · EU/UK" },
    { label: "Billie", value: "billie", meta: "BNPL · DE/AT/SE" },
    { label: "Stablecoins", value: "crypto_currency", meta: "Crypto · Global" },
    { label: "Pix", value: "pix", meta: "Bank Transfer · Brazil" },
    { label: "WeChat Pay", value: "we_chat_pay", meta: "Wallet · China" },
  ];
}

function appendCsvValue(current: string, next: string): string {
  const values = current
    .split(",")
    .map((item) => item.trim())
    .filter(Boolean);
  if (!values.includes(next)) values.push(next);
  return values.join(",");
}

function timeZoneOptions(defaultSuffix: string) {
  const options: { value: number; label: string }[] = [];
  for (let hour = -12; hour <= 14; hour += 1) {
    const value = hour * 60;
    options.push({ value, label: `${formatUtcOffset(value)}${value === 480 ? `（${defaultSuffix}）` : ""}` });
  }
  return options;
}

const FOOTER_LINKS_LIMIT = 24;
const FALLBACK_FOOTER_ICONS = ["link", "twitter", "github", "globe", "book", "activity", "scroll"];

function cloneFooterLink(item: FooterLinkItem): FooterLinkItem {
  return {
    labelZh: item.labelZh ?? "",
    labelEn: item.labelEn ?? "",
    url: item.url ?? "",
    icon: item.icon ?? "link",
  };
}

function footerLinkListsEqual(a: FooterLinkItem[], b: FooterLinkItem[]): boolean {
  if (a.length !== b.length) return false;
  for (let i = 0; i < a.length; i++) {
    if (a[i].labelZh !== b[i].labelZh) return false;
    if (a[i].labelEn !== b[i].labelEn) return false;
    if (a[i].url !== b[i].url) return false;
    if (a[i].icon !== b[i].icon) return false;
  }
  return true;
}

type FooterLinksText = (typeof copy)["zh"]["admin"]["settings"]["footerLinks"] | (typeof copy)["en"]["admin"]["settings"]["footerLinks"];

function FooterLinksEditor({
  items,
  iconChoices,
  errors,
  dirty,
  saving,
  text,
  onAdd,
  onReset,
  onSave,
  onChangeItem,
  onMove,
  onRemove,
}: {
  items: FooterLinkItem[];
  iconChoices: string[];
  errors: Record<number, string>;
  dirty: boolean;
  saving: boolean;
  text: FooterLinksText;
  onAdd: () => void;
  onReset: () => void;
  onSave: () => void;
  onChangeItem: (idx: number, patch: Partial<FooterLinkItem>) => void;
  onMove: (idx: number, delta: number) => void;
  onRemove: (idx: number) => void;
}) {
  const atLimit = items.length >= FOOTER_LINKS_LIMIT;
  return (
    <div className="mt-6 rounded-2xl border-2 border-slate-200 bg-amber-50 p-5">
      <div className="mb-3 flex flex-wrap items-start justify-between gap-3">
        <div className="min-w-0">
          <div className="font-display text-lg font-extrabold">{text.title}</div>
          <p className="mt-1 text-xs text-slate-600">{text.subtitle}</p>
          <div className="mt-1 text-xs font-bold">
            {dirty ? (
              <span className="text-amber-700">{text.unsavedSummary(items.length)}</span>
            ) : (
              <span className="text-emerald-700">{text.noUnsaved}</span>
            )}
            <span className="ml-3 text-slate-500">{text.countSummary(items.length, FOOTER_LINKS_LIMIT)}</span>
          </div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <button
            type="button"
            onClick={onAdd}
            disabled={atLimit}
            className="rounded-full border-2 border-slate-800 bg-white px-3 py-1.5 text-xs font-extrabold disabled:opacity-50 hover:bg-amber-300"
          >
            {atLimit ? text.limitReached : text.add}
          </button>
          <button
            type="button"
            onClick={onReset}
            disabled={!dirty || saving}
            className="rounded-full border-2 border-slate-800 bg-white px-3 py-1.5 text-xs font-extrabold disabled:opacity-50 hover:bg-amber-300"
          >
            {text.reset}
          </button>
          <button
            type="button"
            onClick={onSave}
            disabled={!dirty || saving}
            className="rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-1.5 text-xs font-extrabold text-white disabled:opacity-50"
          >
            {text.save}
          </button>
        </div>
      </div>
      {items.length === 0 ? (
        <div className="rounded-2xl border-2 border-dashed border-slate-300 bg-white p-6 text-center text-sm text-slate-500">
          {text.empty}
        </div>
      ) : (
        <ul className="grid gap-2">
          {items.map((item, idx) => {
            const error = errors[idx];
            return (
              <li
                key={idx}
                className={`grid gap-2 rounded-2xl border-2 bg-white p-3 md:grid-cols-[120px_1fr_1fr_minmax(0,1.4fr)_auto] md:items-center ${error ? "border-pink-500" : "border-slate-200"}`}
              >
                <select
                  value={item.icon || "link"}
                  onChange={(e) => onChangeItem(idx, { icon: e.target.value })}
                  aria-label={text.colIcon}
                  className="rounded-xl border-2 border-slate-300 bg-amber-50 px-2 py-1.5 text-xs font-bold focus:bg-white focus:border-violet-500 outline-none"
                >
                  {iconChoices.map((key) => (
                    <option key={key} value={key}>{key}</option>
                  ))}
                </select>
                <input
                  value={item.labelZh}
                  placeholder={text.labelZhPlaceholder}
                  aria-label={text.colLabelZh}
                  onChange={(e) => onChangeItem(idx, { labelZh: e.target.value })}
                  className="rounded-xl border-2 border-slate-300 bg-amber-50 px-3 py-1.5 text-sm focus:bg-white focus:border-violet-500 outline-none"
                />
                <input
                  value={item.labelEn}
                  placeholder={text.labelEnPlaceholder}
                  aria-label={text.colLabelEn}
                  onChange={(e) => onChangeItem(idx, { labelEn: e.target.value })}
                  className="rounded-xl border-2 border-slate-300 bg-amber-50 px-3 py-1.5 text-sm focus:bg-white focus:border-violet-500 outline-none"
                />
                <input
                  value={item.url}
                  placeholder={text.urlPlaceholder}
                  aria-label={text.colUrl}
                  onChange={(e) => onChangeItem(idx, { url: e.target.value })}
                  className="rounded-xl border-2 border-slate-300 bg-amber-50 px-3 py-1.5 font-mono text-xs focus:bg-white focus:border-violet-500 outline-none"
                />
                <div className="flex items-center justify-end gap-1">
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={() => onMove(idx, -1)}
                        disabled={idx === 0}
                        aria-label={text.moveUp}
                        className="rounded-full border-2 border-slate-300 bg-white px-2 py-1 text-xs font-bold disabled:opacity-30 hover:bg-amber-100"
                      >
                        ↑
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>{text.moveUp}</TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={() => onMove(idx, 1)}
                        disabled={idx === items.length - 1}
                        aria-label={text.moveDown}
                        className="rounded-full border-2 border-slate-300 bg-white px-2 py-1 text-xs font-bold disabled:opacity-30 hover:bg-amber-100"
                      >
                        ↓
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>{text.moveDown}</TooltipContent>
                  </Tooltip>
                  <Tooltip>
                    <TooltipTrigger asChild>
                      <button
                        type="button"
                        onClick={() => onRemove(idx)}
                        aria-label={text.remove}
                        className="rounded-full border-2 border-pink-400 bg-white px-2 py-1 text-xs font-bold text-pink-600 hover:bg-pink-100"
                      >
                        ✕
                      </button>
                    </TooltipTrigger>
                    <TooltipContent>{text.remove}</TooltipContent>
                  </Tooltip>
                </div>
                {error && (
                  <div className="md:col-span-5 text-xs font-bold text-pink-600">{error}</div>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </div>
  );
}

function LedgerEntriesTab() { return <SimpleTable path="/v1/admin/ledger" empty="账本暂无记录" />; }
function MoneyEventsTab() { return <SimpleTable path="/v1/admin/money-events" empty="尚无资金事件" />; }
function AuditTab() { return <SimpleTable path="/v1/admin/audit" empty="尚无审计记录" />; }

function SimpleTable({ path, empty }: { path: string; empty: string }) {
  const [items, setItems] = useState<AnyRow[] | null>(null);
  const formatDate = useDateTimeFormatter();
  useEffect(() => { apiGetAllItems<AnyRow>(path).then(setItems).catch(() => setItems([])); }, [path]);
  const columns = inferColumns(items?.[0], formatDate);
  return (
    <AdminDataTable
      rows={items ?? []}
      loading={items === null}
      rowKey={(r, i) => String(r.id ?? r.event_id ?? r.request_id ?? i)}
      empty={<EmptyState shape="circle" title={empty} />}
      columns={columns}
    />
  );
}

function inferColumns(sample?: AnyRow, formatDate?: (value?: string | null) => string): Column<AnyRow>[] {
  if (!sample) return [{ key: "preview", header: "数据", render: () => <span /> }];
  const keys = Object.keys(sample).slice(0, 8);
  return keys.map<Column<AnyRow>>((k) => ({
    key: k,
    header: k,
    render: (row) => <span className="break-all font-mono text-xs">{formatCell(row[k], k, formatDate)}</span>
  }));
}

function formatCell(v: unknown, key?: string, formatDateValue?: (value?: string | null) => string): string {
  if (v === null || v === undefined) return "—";
  if (typeof v === "string" && key && isTimeColumn(key)) return formatDateValue ? formatDateValue(v) : formatDateTime(v);
  if (typeof v === "object") return JSON.stringify(v);
  return String(v);
}

function isTimeColumn(key: string): boolean {
  return key.endsWith("_at") || key === "created" || key === "updated" || key === "time";
}
