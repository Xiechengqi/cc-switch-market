"use client";

import { useCallback, useEffect, useState } from "react";
import { Banknote, FileText, Paperclip, Wallet, X } from "lucide-react";
import { useMarketAuth } from "@/components/auth";
import { PageHeader } from "@/components/ui/PageHeader";
import { StatCard } from "@/components/ui/StatCard";
import { Pill } from "@/components/ui/Pill";
import { Modal, ModalActions } from "@/components/ui/Modal";
import { DataTable, type DataTableProps } from "@/components/ui/DataTable";
import { EmptyState } from "@/components/ui/EmptyState";
import { Skeleton } from "@/components/ui/Skeleton";
import { MoneyAmount } from "@/components/ui/MoneyAmount";
import { useToast } from "@/components/ui/Toast";
import { apiGet, apiGetPage, apiPost, apiPutBytes } from "@/lib/client-api";
import { useLocale } from "@/components/language-provider";
import { copy } from "@/lib/copy";
import { commissionText, usePublicConfig } from "@/lib/public-config";
import { useDateTimeFormatter } from "@/lib/time";

type ClaimSummary = {
  available_usd: string;
  pending_usd: string;
  paid_usd: string;
  minimum_payout_usd?: string;
  can_payout: boolean;
  owner_email?: string;
};

type Preview = {
  gross_amount_usd: string;
  payout_fee_usd: string;
  net_payout_usd: string;
};

type PayoutItem = Record<string, unknown> & {
  id?: string;
  method?: string;
  status?: string;
  amount_usd?: string;
  payout_fee_usd?: string;
  net_payout_usd?: string;
  external_tx_id?: string;
  created_at?: string;
  gateio_batch_id?: string;
  failure_reason?: string;
  params_json?: {
    sourceOwnerEmail?: string | null;
    targetOwnerEmail?: string | null;
    requestId?: string | null;
  } | null;
};

type ImageAttachment = {
  id?: string;
  original_filename?: string;
  download_url?: string;
  object_key?: string;
};

const USER_TABLE_PAGE_SIZE_KEY = "cc-switch-market:user-table-page-size";
const USER_TABLE_PAGE_SIZE_OPTIONS = [10, 20, 50, 100];

function ClaimDataTable<T>(props: DataTableProps<T>) {
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

type PagedRows<T> = {
  items: T[] | null;
  hasMore: boolean;
  loadingMore: boolean;
  loadMore: () => void;
};

function usePagedRows<T>(path: string, limit = 50): PagedRows<T> {
  const [items, setItems] = useState<T[] | null>(null);
  const [nextCursor, setNextCursor] = useState<string | null>(null);
  const [hasMore, setHasMore] = useState(false);
  const [loadingMore, setLoadingMore] = useState(false);

  const loadPage = useCallback(async (cursor: string | null, append: boolean) => {
    if (append) setLoadingMore(true);
    else setItems(null);
    try {
      const page = await apiGetPage<T>(path, { limit, cursor });
      setItems((prev) => append ? [...(prev ?? []), ...(page.items ?? [])] : (page.items ?? []));
      setNextCursor(page.nextCursor ?? null);
      setHasMore(Boolean(page.hasMore));
    } catch {
      if (!append) setItems([]);
      setNextCursor(null);
      setHasMore(false);
    } finally {
      if (append) setLoadingMore(false);
    }
  }, [limit, path]);

  const reload = useCallback(() => { void loadPage(null, false); }, [loadPage]);
  const loadMore = useCallback(() => {
    if (!hasMore || loadingMore) return;
    void loadPage(nextCursor, true);
  }, [hasMore, loadPage, loadingMore, nextCursor]);

  useEffect(() => { reload(); }, [reload]);

  return { items, hasMore, loadingMore, loadMore };
}

function PagedClaimDataTable<T>({ page, ...props }: Omit<DataTableProps<T>, "rows" | "loading"> & { page: PagedRows<T> }) {
  const { locale } = useLocale();
  return (
    <div className="grid gap-3">
      <ClaimDataTable {...props} rows={page.items ?? []} loading={page.items === null} />
      {page.hasMore && (
        <div className="flex justify-center">
          <button
            type="button"
            onClick={page.loadMore}
            disabled={page.loadingMore}
            className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 text-sm font-bold lift disabled:opacity-50"
          >
            {page.loadingMore ? (locale === "zh" ? "加载中..." : "Loading...") : (locale === "zh" ? "加载更多" : "Load more")}
          </button>
        </div>
      )}
    </div>
  );
}

export function ClaimRoot() {
  const { user, showLogin } = useMarketAuth();
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const publicConfig = usePublicConfig();
  const commission = commissionText(locale, publicConfig.totalCommissionBps);
  const [summary, setSummary] = useState<ClaimSummary | null>(null);
  const [openGateio, setOpenGateio] = useState(false);
  const [openManual, setOpenManual] = useState(false);
  const [openBalance, setOpenBalance] = useState(false);
  const [openProviderTransfer, setOpenProviderTransfer] = useState(false);

  function reload() {
    apiGet<ClaimSummary>("/v1/provider/claim/summary").then(setSummary).catch(() => setSummary({ available_usd: "0", pending_usd: "0", paid_usd: "0", can_payout: false }));
  }
  useEffect(() => { reload(); }, []);

  if (!user) {
    return (
      <div className="grid gap-6">
        <PageHeader title={c.title} />
        <div className="sticker bg-amber-50 p-8 text-center">
          <div className="font-display text-2xl font-extrabold">{c.anonTitle}</div>
          <p className="mt-2 text-slate-600">{c.anonHint}</p>
          <button onClick={showLogin} className="mt-4 inline-flex rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">{c.openLogin}</button>
        </div>
      </div>
    );
  }

  const minimum = summary?.minimum_payout_usd ?? "1.00";
  const available = Number(summary?.available_usd ?? 0);
  const min = Number(minimum);
  const canPayout = !!summary?.can_payout && available >= min;
  const canInternalTransfer = available > 0;
  const diff = (min - available).toFixed(2);

  return (
    <div className="grid gap-6">
      <PageHeader
        title={c.title}
        subtitle={summary?.owner_email ? <span>{c.subtitleEmail(summary.owner_email)}</span> : c.subtitleAnon}
      />

      <div className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 text-sm font-bold text-slate-700">
        {commission.claimNote}
      </div>

      <div className="grid gap-4 sm:grid-cols-3">
        <StatCard label={c.statAvailable} value={summary ? `$${summary.available_usd}` : <Skeleton />} color="violet" icon={<Wallet size={16} />} loading={!summary} />
        <StatCard label={c.statLocked} value={summary ? `$${summary.pending_usd}` : <Skeleton />} color="amber" icon={<Banknote size={16} />} loading={!summary} />
        <StatCard label={c.statPaid} value={summary ? `$${summary.paid_usd}` : <Skeleton />} color="emerald" icon={<FileText size={16} />} loading={!summary} />
      </div>

      {!canPayout && summary && (
        <EmptyState
          shape="blob"
          title={c.thresholdTitle(diff)}
          hint={c.thresholdHint(minimum)}
        />
      )}

      {canPayout && (
        <div className="grid gap-5 md:grid-cols-2">
          <div className="sticker bg-emerald-50 p-6 lift">
            <div className="flex items-center gap-3">
              <span className="rounded-full border-2 border-slate-800 bg-emerald-400 p-2 text-white"><Banknote size={18} /></span>
              <h3 className="font-display text-xl font-extrabold">{c.gateioTitle}</h3>
              <a
                href="https://www.gate.com/zh/referral/earn-together/invite/X1AVBFpX?ref=X1AVBFpX&ref_type=103&utm_cmp=rXJBDjtJ&activity_id=1776947564884"
                target="_blank"
                rel="noopener noreferrer"
                className="text-xs font-bold text-violet-600 underline decoration-2 underline-offset-2"
              >
                {c.gateioRegister}
              </a>
            </div>
            <p className="mt-2 text-sm text-slate-600">{c.gateioBody}</p>
            <button onClick={() => setOpenGateio(true)} className="mt-4 inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">
              {c.gateioBtn}
            </button>
          </div>
          <div className="sticker bg-amber-50 p-6 lift">
            <div className="flex items-center gap-3">
              <span className="rounded-full border-2 border-slate-800 bg-amber-300 p-2"><FileText size={18} /></span>
              <h3 className="font-display text-xl font-extrabold">{c.manualTitle}</h3>
            </div>
            <p className="mt-2 text-sm text-slate-600">{c.manualBody}</p>
            <button onClick={() => setOpenManual(true)} className="mt-4 inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">
              {c.manualBtn}
            </button>
          </div>
        </div>
      )}

      {canInternalTransfer && (
        <div className="grid gap-5 md:grid-cols-2">
          <div className="sticker bg-violet-50 p-6 lift">
            <div className="flex items-center gap-3">
              <span className="rounded-full border-2 border-slate-800 bg-violet-500 p-2 text-white"><Wallet size={18} /></span>
              <h3 className="font-display text-xl font-extrabold">{c.balanceTitle}</h3>
            </div>
            <p className="mt-2 text-sm text-slate-600">{c.balanceBody}</p>
            <button onClick={() => setOpenBalance(true)} className="mt-4 inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">
              {c.balanceBtn}
            </button>
          </div>
          <div className="sticker bg-pink-50 p-6 lift">
            <div className="flex items-center gap-3">
              <span className="rounded-full border-2 border-slate-800 bg-pink-300 p-2"><Banknote size={18} /></span>
              <h3 className="font-display text-xl font-extrabold">{c.providerTransferTitle}</h3>
            </div>
            <p className="mt-2 text-sm text-slate-600">{c.providerTransferBody}</p>
            <button onClick={() => setOpenProviderTransfer(true)} className="mt-4 inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">
              {c.providerTransferBtn}
            </button>
          </div>
        </div>
      )}

      <div className="sticker bg-white p-6">
        <h2 className="font-display text-2xl font-extrabold">{c.payoutsTitle}</h2>
        <div className="mt-4">
          <PayoutTable />
        </div>
      </div>

      <div className="sticker bg-white p-6">
        <h2 className="font-display text-2xl font-extrabold">{c.earningsTitle}</h2>
        <p className="text-sm text-slate-500">{c.earningsSubtitle(commission.rate)}</p>
        <div className="mt-4">
          <EarningsTable />
        </div>
      </div>

      <GateioPayoutModal
        open={openGateio}
        onClose={() => { setOpenGateio(false); reload(); }}
        max={summary?.available_usd ?? "0"}
      />
      <ManualPayoutModal
        open={openManual}
        onClose={() => { setOpenManual(false); reload(); }}
        max={summary?.available_usd ?? "0"}
      />
      <ConvertToBalanceModal
        open={openBalance}
        onClose={() => { setOpenBalance(false); reload(); }}
        max={summary?.available_usd ?? "0"}
      />
      <TransferProviderModal
        open={openProviderTransfer}
        onClose={() => { setOpenProviderTransfer(false); reload(); }}
        max={summary?.available_usd ?? "0"}
      />
    </div>
  );
}

function PreviewBox({ value }: { value: Preview | null }) {
  if (!value) return null;
  return (
    <div className="rounded-2xl border-2 border-slate-800 bg-white p-4">
      <MoneyAmount gross={value.gross_amount_usd} fee={value.payout_fee_usd} net={value.net_payout_usd} />
    </div>
  );
}

function GateioPayoutModal({ open, onClose, max }: { open: boolean; onClose: () => void; max: string }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const [amount, setAmount] = useState(max);
  const [uid, setUid] = useState("");
  const [preview, setPreview] = useState<Preview | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => { if (open) { setAmount(integerPayoutMax(max)); setUid(""); } }, [open, max]);

  useEffect(() => {
    if (!open || !isValidIntegerPayoutAmount(amount, max, 1)) { setPreview(null); return; }
    apiGet<Preview>(`/v1/provider/claim/payout-preview?method=gateio&amount_usd=${encodeURIComponent(amount)}`).then(setPreview).catch(() => setPreview(null));
  }, [amount, max, open]);

  async function submit() {
    if (!isValidIntegerPayoutAmount(amount, max, 1)) {
      toast.push({ variant: "error", title: c.payoutInvalidAmount, description: c.gateioAmountHint });
      return;
    }
    if (!isGateioUid(uid)) { toast.push({ variant: "error", title: c.submitInvalidUid }); return; }
    setSubmitting(true);
    try {
      await apiPost("/v1/provider/claim/payout", {
        params: { uid: uid.trim() },
        amount_usd: amount
      });
      toast.push({ variant: "success", title: c.submitSuccess, description: c.submitSuccessDesc });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.submitFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={c.gateioModalTitle} description={c.gateioModalDesc} width="md">
      <div className="grid gap-4">
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.gateioAmountLabel}</span>
          <input type="number" min="1" step="1" value={amount} onChange={(e) => setAmount(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 text-xl font-bold outline-none focus:bg-white" />
          <span className="text-xs text-slate-500">{c.gateioMaxHint(integerPayoutMax(max))} · {c.gateioAmountHint}</span>
        </label>
        <PreviewBox value={preview} />
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.gateioUidLabel}</span>
          <input value={uid} onChange={(e) => setUid(e.target.value)} placeholder={c.gateioUidPlaceholder} inputMode="numeric" className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
          <span className="text-xs text-slate-500">{c.gateioUidHint}</span>
        </label>
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.submitCancel}</button>
        <button onClick={submit} disabled={submitting} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">
          {submitting ? c.submitSubmitting : c.submitSubmit}
        </button>
      </ModalActions>
    </Modal>
  );
}

function ManualPayoutModal({ open, onClose, max }: { open: boolean; onClose: () => void; max: string }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const [amount, setAmount] = useState(max);
  const [text, setText] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const [preview, setPreview] = useState<Preview | null>(null);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => { if (open) { setAmount(integerPayoutMax(max)); setText(""); setFiles([]); } }, [open, max]);
  useEffect(() => {
    if (!open || !isValidIntegerPayoutAmount(amount, max, 20)) { setPreview(null); return; }
    apiGet<Preview>(`/v1/provider/claim/payout-preview?method=manual&amount_usd=${encodeURIComponent(amount)}`).then(setPreview).catch(() => setPreview(null));
  }, [amount, max, open]);

  async function submit() {
    if (!isValidIntegerPayoutAmount(amount, max, 20)) {
      toast.push({ variant: "error", title: c.payoutInvalidAmount, description: c.manualAmountHint });
      return;
    }
    if (!text.trim()) { toast.push({ variant: "error", title: c.manualMissingDetails }); return; }
    setSubmitting(true);
    try {
      const attachmentIds: string[] = [];
      for (const file of files) {
        const presigned = await apiPost<{ attachment_id: string; upload_url: string }>("/v1/ticket-attachments/presign", {
          filename: file.name, content_type: file.type || "application/octet-stream", byte_size: file.size
        });
        await apiPutBytes(presigned.upload_url, file);
        attachmentIds.push(presigned.attachment_id);
      }
      await apiPost("/v1/provider/claim/payout-ticket", {
        amount_usd: amount,
        payout_details_text: text.trim(),
        attachment_ids: attachmentIds
      });
      toast.push({ variant: "success", title: c.manualSuccess, description: c.manualSuccessDesc });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.submitFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={c.manualModalTitle} description={c.manualModalDesc} width="lg">
      <div className="grid gap-4">
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.manualAmountLabel}</span>
          <input type="number" min="20" step="1" value={amount} onChange={(e) => setAmount(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 text-xl font-bold outline-none focus:bg-white" />
          <span className="text-xs text-slate-500">{c.gateioMaxHint(integerPayoutMax(max))} · {c.manualAmountHint}</span>
        </label>
        <PreviewBox value={preview} />
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.manualDetailsLabel}</span>
          <textarea value={text} onChange={(e) => setText(e.target.value)} className="min-h-32 rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" placeholder={c.manualDetailsPlaceholder} />
        </label>
        <FilePicker files={files} onChange={setFiles} />
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.submitCancel}</button>
        <button onClick={submit} disabled={submitting} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">
          {submitting ? c.submitSubmitting : c.manualSubmit}
        </button>
      </ModalActions>
    </Modal>
  );
}

function ConvertToBalanceModal({ open, onClose, max }: { open: boolean; onClose: () => void; max: string }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const [amount, setAmount] = useState(max);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => { if (open) setAmount(max); }, [open, max]);

  async function submit() {
    if (!isValidTransferAmount(amount, max)) {
      toast.push({ variant: "error", title: c.internalTransferInvalidAmount, description: c.internalTransferInvalidAmountDesc });
      return;
    }
    setSubmitting(true);
    try {
      await apiPost("/v1/provider/claim/convert-to-balance", { amount_usd: amount });
      toast.push({ variant: "success", title: c.balanceSuccess });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.internalTransferFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={c.balanceModalTitle} description={c.balanceModalDesc} width="md">
      <div className="grid gap-4">
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.balanceAmountLabel}</span>
          <input type="number" min="0.01" step="0.01" value={amount} onChange={(e) => setAmount(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 text-xl font-bold outline-none focus:bg-white" />
          <span className="text-xs text-slate-500">{c.gateioMaxHint(max)}</span>
        </label>
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.submitCancel}</button>
        <button onClick={submit} disabled={submitting} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">
          {submitting ? c.submitSubmitting : c.balanceSubmit}
        </button>
      </ModalActions>
    </Modal>
  );
}

function TransferProviderModal({ open, onClose, max }: { open: boolean; onClose: () => void; max: string }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const [amount, setAmount] = useState(max);
  const [email, setEmail] = useState("");
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => { if (open) { setAmount(max); setEmail(""); } }, [open, max]);

  async function submit() {
    if (!isValidTransferAmount(amount, max)) {
      toast.push({ variant: "error", title: c.internalTransferInvalidAmount, description: c.internalTransferInvalidAmountDesc });
      return;
    }
    if (!looksLikeEmail(email)) {
      toast.push({ variant: "error", title: c.internalTransferInvalidEmail });
      return;
    }
    setSubmitting(true);
    try {
      await apiPost("/v1/provider/claim/transfer-provider", { amount_usd: amount, target_owner_email: email.trim() });
      toast.push({ variant: "success", title: c.providerTransferSuccess });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.internalTransferFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={c.providerTransferModalTitle} description={c.providerTransferModalDesc} width="md">
      <div className="grid gap-4">
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.providerTransferAmountLabel}</span>
          <input type="number" min="0.01" step="0.01" value={amount} onChange={(e) => setAmount(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 text-xl font-bold outline-none focus:bg-white" />
          <span className="text-xs text-slate-500">{c.gateioMaxHint(max)}</span>
        </label>
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.providerTransferEmailLabel}</span>
          <input type="email" value={email} onChange={(e) => setEmail(e.target.value)} placeholder={c.providerTransferEmailPlaceholder} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
        </label>
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.submitCancel}</button>
        <button onClick={submit} disabled={submitting} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">
          {submitting ? c.submitSubmitting : c.providerTransferSubmit}
        </button>
      </ModalActions>
    </Modal>
  );
}

export function FilePicker({ files, onChange, accept = "image/*" }: { files: File[]; onChange: (next: File[]) => void; accept?: string }) {
  const { locale } = useLocale();
  const c = copy[locale].claim;
  function handleFiles(next: File[]) {
    const filtered = next.filter((file) => file.type.startsWith("image/") && file.size <= 2 * 1024 * 1024);
    onChange(filtered);
  }

  return (
    <div>
      <label className="flex cursor-pointer items-center justify-center gap-2 rounded-2xl border-2 border-dashed border-slate-400 bg-white p-4 text-sm font-bold text-slate-500 hover:border-slate-800 hover:bg-amber-50">
        <Paperclip size={16} /> {c.filePickerLabel}
        <input type="file" multiple accept={accept} className="hidden" onChange={(e) => handleFiles(Array.from(e.currentTarget.files ?? []))} />
      </label>
      {files.length > 0 && (
        <ul className="mt-2 grid gap-1 text-xs">
          {files.map((f, i) => (
            <li key={i} className="flex items-center justify-between rounded-xl border border-slate-300 bg-white px-3 py-1.5 font-mono">
              <span className="truncate">{f.name}</span>
              <span className="text-slate-500">{Math.round(f.size / 1024)} KB</span>
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

export function ImageAttachmentGrid({ attachments }: { attachments: ImageAttachment[] }) {
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const [lightboxIndex, setLightboxIndex] = useState<number | null>(null);
  if (!attachments.length) return null;
  const active = lightboxIndex !== null ? attachments[lightboxIndex] : null;
  return (
    <>
      <div className="grid gap-2">
        <div className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.attachmentsTitle}</div>
        <div className="grid gap-3 sm:grid-cols-2">
          {attachments.map((attachment, index) => (
            <button
              key={String(attachment.id ?? index)}
              type="button"
              onClick={() => setLightboxIndex(index)}
              className="overflow-hidden rounded-2xl border-2 border-slate-800 bg-white text-left lift"
            >
              <img
                src={attachment.download_url ?? ""}
                alt={attachment.original_filename ?? attachment.object_key ?? c.attachmentFallback}
                className="block h-48 w-full object-cover"
              />
              <div className="border-t-2 border-slate-800 px-3 py-2 text-xs font-bold text-slate-600">
                {attachment.original_filename ?? attachment.object_key ?? c.attachmentFallback}
              </div>
            </button>
          ))}
        </div>
      </div>

      {active && (
        <div className="fixed inset-0 z-[70] flex items-center justify-center bg-slate-900/80 p-4" onClick={() => setLightboxIndex(null)}>
          <div className="relative max-h-[92vh] max-w-[92vw]" onClick={(e) => e.stopPropagation()}>
            <button
              type="button"
              onClick={() => setLightboxIndex(null)}
              aria-label={c.lightboxClose}
              className="absolute right-2 top-2 rounded-full border-2 border-white bg-slate-900/80 p-2 text-white"
            >
              <X size={18} />
            </button>
            <img
              src={active.download_url ?? ""}
              alt={active.original_filename ?? active.object_key ?? c.attachmentFallback}
              className="max-h-[85vh] max-w-[90vw] rounded-2xl border-2 border-white object-contain bg-black"
            />
            <div className="mt-3 text-center text-sm font-bold text-white">
              {active.original_filename ?? active.object_key ?? c.attachmentFallback}
            </div>
          </div>
        </div>
      )}
    </>
  );
}

function PayoutTable() {
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const page = usePagedRows<PayoutItem>("/v1/provider/claim/payouts");
  const formatDate = useDateTimeFormatter();
  return (
    <PagedClaimDataTable
      page={page}
      rowKey={(r, i) => String(r.id ?? i)}
      empty={<EmptyState shape="square" title={c.payoutsEmptyTitle} hint={c.payoutsEmptyHint} />}
      columns={[
        { key: "method", header: c.payoutCol.method, render: (r) => <span className="font-bold">{payoutMethodLabel(r.method, c)}</span> },
        { key: "amount", header: c.payoutCol.amount, render: (r) => <MoneyAmount gross={r.amount_usd} fee={r.payout_fee_usd} net={r.net_payout_usd} layout="inline" /> },
        { key: "status", header: c.payoutCol.status, render: (r) => <Pill status={r.status ?? "pending"} /> },
        { key: "external", header: c.payoutCol.external, render: (r) => <span className="font-mono text-xs text-slate-500 break-all">{payoutProof(r)}</span> },
        { key: "time", header: c.payoutCol.time, render: (r) => <span className="text-xs text-slate-500">{formatDate(r.created_at)}</span> }
      ]}
      expandable={(r) =>
        r.failure_reason ? (
          <div className="text-sm text-pink-700"><b>{c.failureLabel}</b> {r.failure_reason}</div>
        ) : null
      }
    />
  );
}

function isValidTransferAmount(amount: string, max: string) {
  const value = Number(amount);
  const maxValue = Number(max);
  return Number.isFinite(value) && Number.isFinite(maxValue) && value > 0 && value <= maxValue;
}

function integerPayoutMax(max: string) {
  const maxValue = Math.floor(Number(max));
  return Number.isFinite(maxValue) && maxValue > 0 ? String(maxValue) : "0";
}

function isValidIntegerPayoutAmount(amount: string, max: string, min: number) {
  const value = Number(amount);
  const maxValue = Math.floor(Number(max));
  return Number.isInteger(value) && Number.isFinite(maxValue) && value >= min && value <= maxValue;
}

function isGateioUid(value: string) {
  return /^\d+$/.test(value.trim());
}

function looksLikeEmail(value: string) {
  const trimmed = value.trim();
  const [local, domain] = trimmed.split("@");
  return !!local && !!domain && domain.includes(".") && !domain.startsWith(".") && !domain.endsWith(".");
}

type ClaimCopy = (typeof copy)["zh"]["claim"] | (typeof copy)["en"]["claim"];

function payoutMethodLabel(method: string | undefined, c: ClaimCopy) {
  switch (method) {
    case "gateio": return c.payoutMethodGateio;
    case "manual": return c.payoutMethodManual;
    case "balance": return c.payoutMethodBalance;
    case "provider": return c.payoutMethodProvider;
    case "provider_received": return c.payoutMethodProviderReceived;
    case "router_commission": return c.payoutMethodRouterCommission;
    default: return method || "-";
  }
}

function payoutProof(item: PayoutItem) {
  if (item.method === "router_commission" && item.params_json?.requestId) {
    return item.params_json.requestId;
  }
  if (item.method === "provider_received" && item.params_json?.sourceOwnerEmail) {
    return item.params_json.sourceOwnerEmail;
  }
  if (item.method === "provider" && item.params_json?.targetOwnerEmail) {
    return item.params_json.targetOwnerEmail;
  }
  return item.external_tx_id || item.gateio_batch_id || "—";
}

function EarningsTable() {
  const { locale } = useLocale();
  const c = copy[locale].claim;
  const page = usePagedRows<Record<string, unknown>>("/v1/provider/earnings");
  return (
    <PagedClaimDataTable
      page={page}
      rowKey={(r, i) => String(r.request_id ?? i)}
      empty={<EmptyState shape="circle" title={c.earningsEmptyTitle} hint={c.earningsEmptyHint} />}
      columns={[
        { key: "rid", header: c.earningsCol.request, render: (r) => <span className="font-mono text-xs break-all">{String(r.request_id ?? "")}</span> },
        { key: "model", header: c.earningsCol.model, render: (r) => <span className="font-mono text-sm">{String(r.model ?? "")}</span> },
        { key: "status", header: c.earningsCol.status, render: (r) => <Pill status={String(r.status ?? "pending")} /> },
        { key: "amount", header: c.earningsCol.amount, render: (r) => <MoneyAmount gross={String(r.gross_amount ?? r.usage_amount ?? "0")} fee={String(r.fee_amount ?? "0")} net={String(r.net_amount ?? "0")} layout="inline" /> }
      ]}
    />
  );
}
