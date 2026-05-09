"use client";

import { useEffect, useMemo, useState } from "react";
import { ArrowLeft, PlusCircle, Send, Trash2, XCircle } from "lucide-react";
import { PageHeader } from "@/components/ui/PageHeader";
import { Pill } from "@/components/ui/Pill";
import { Modal, ModalActions } from "@/components/ui/Modal";
import { EmptyState } from "@/components/ui/EmptyState";
import { Skeleton } from "@/components/ui/Skeleton";
import { useToast } from "@/components/ui/Toast";
import { FilePicker, ImageAttachmentGrid } from "@/app/claim/ui";
import { Page, apiDelete, apiGet, apiPost, apiPutBytes } from "@/lib/client-api";
import { useLocale } from "@/components/language-provider";
import { copy } from "@/lib/copy";
import { useMarketAuth } from "@/components/auth";
import { useDateTimeFormatter } from "@/lib/time";

type Ticket = {
  id?: string;
  ticket_no?: string;
  ticket_type?: string;
  subject?: string;
  status?: string;
  priority?: string;
  created_at?: string;
  can_close?: boolean;
  can_delete?: boolean;
};

type Message = { id?: string; sender_type?: string; body_text?: string; created_at?: string; internal_note?: boolean };
type Attachment = { id?: string; original_filename?: string; object_key?: string; download_url?: string };

const SUPPORT_FILTER_STORAGE_KEY = "cc-switch-market:support-filters";

type SupportFilters = {
  viewFilter: string;
  statusFilter: string;
  priorityFilter: string;
};

function readStoredSupportFilters(): SupportFilters {
  const fallback = { viewFilter: "all", statusFilter: "all", priorityFilter: "all" };
  if (typeof window === "undefined") return fallback;
  try {
    const raw = window.localStorage.getItem(SUPPORT_FILTER_STORAGE_KEY);
    if (!raw) return fallback;
    const parsed = JSON.parse(raw) as Partial<SupportFilters>;
    return {
      viewFilter: validSupportValue(parsed.viewFilter, ["all", "mine_to_reply", "waiting_platform", "done"], fallback.viewFilter),
      statusFilter: validSupportValue(parsed.statusFilter, ["all", "open", "waiting_user", "waiting_admin", "resolved", "closed"], fallback.statusFilter),
      priorityFilter: validSupportValue(parsed.priorityFilter, ["all", "low", "normal", "high", "urgent"], fallback.priorityFilter),
    };
  } catch {
    return fallback;
  }
}

function validSupportValue(value: unknown, allowed: string[], fallback: string): string {
  return typeof value === "string" && allowed.includes(value) ? value : fallback;
}

function priorityVariant(priority?: string) {
  switch (priority) {
    case "low": return "neutral";
    case "high": return "warning";
    case "urgent": return "failed";
    default: return "info";
  }
}

export function SupportRoot() {
  const { locale } = useLocale();
  const c = copy[locale].support;
  const { user, loading: authLoading, showLogin } = useMarketAuth();
  const formatDate = useDateTimeFormatter();
  const [tickets, setTickets] = useState<Ticket[] | null>(null);
  const [activeId, setActiveId] = useState<string | null>(null);
  const [createOpen, setCreateOpen] = useState(false);
  const [storedFilters] = useState(readStoredSupportFilters);
  const [statusFilter, setStatusFilter] = useState<string>(storedFilters.statusFilter);
  const [priorityFilter, setPriorityFilter] = useState<string>(storedFilters.priorityFilter);
  const [viewFilter, setViewFilter] = useState<string>(storedFilters.viewFilter);

  function reload() {
    apiGet<Page<Ticket>>("/v1/tickets").then((p) => setTickets(p.items)).catch(() => setTickets([]));
  }
  useEffect(() => {
    if (authLoading) return;
    if (!user) {
      setTickets([]);
      setActiveId(null);
      setCreateOpen(false);
      return;
    }
    reload();
  }, [authLoading, user]);
  useEffect(() => {
    if (typeof window === "undefined" || !user) return;
    const params = new URLSearchParams(window.location.search);
    const ticketId = params.get("ticket");
    if (ticketId) setActiveId(ticketId);
  }, [user]);
  useEffect(() => {
    window.localStorage.setItem(SUPPORT_FILTER_STORAGE_KEY, JSON.stringify({ viewFilter, statusFilter, priorityFilter }));
  }, [viewFilter, statusFilter, priorityFilter]);

  const activeTicket = tickets?.find((ticket) => ticket.id === activeId) ?? null;
  const filteredTickets = useMemo(() => {
    const source = tickets ?? [];
    return source.filter((ticket) => {
      const matchStatus = statusFilter === "all" || ticket.status === statusFilter;
      const matchPriority = priorityFilter === "all" || ticket.priority === priorityFilter;
      const matchView =
        viewFilter === "all" ||
        (viewFilter === "mine_to_reply" && ticket.status === "waiting_user") ||
        (viewFilter === "waiting_platform" && (ticket.status === "open" || ticket.status === "waiting_admin")) ||
        (viewFilter === "done" && (ticket.status === "closed" || ticket.status === "resolved"));
      return matchStatus && matchPriority && matchView;
    });
  }, [tickets, statusFilter, priorityFilter, viewFilter]);

  const viewCounts = useMemo(() => {
    const source = tickets ?? [];
    return {
      all: source.length,
      mine_to_reply: source.filter((ticket) => ticket.status === "waiting_user").length,
      waiting_platform: source.filter((ticket) => ticket.status === "open" || ticket.status === "waiting_admin").length,
      done: source.filter((ticket) => ticket.status === "closed" || ticket.status === "resolved").length,
    };
  }, [tickets]);

  const activeDetailStatusHint = activeTicket?.status === "waiting_user"
    ? c.statusYourTurn
    : activeTicket?.status === "waiting_admin" || activeTicket?.status === "open"
      ? c.statusOurTurn
      : activeTicket?.status === "closed" || activeTicket?.status === "resolved"
        ? c.statusEnded
        : c.statusViewing;

  return (
    <div className="grid gap-6">
      <PageHeader
        title={activeId ? c.detailTitle : c.title}
        subtitle={activeId ? c.detailSubtitle(activeTicket?.ticket_no ?? "", activeDetailStatusHint) : c.subtitle}
        actions={
          activeId ? (
            <button onClick={() => setActiveId(null)} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">
              <ArrowLeft size={16} /> {c.backToList}
            </button>
          ) : user ? (
            <button onClick={() => setCreateOpen(true)} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">
              <PlusCircle size={16} /> {c.newTicket}
            </button>
          ) : null
        }
      />

      {!authLoading && !user ? (
        <div className="sticker bg-amber-50 p-8 text-center">
          <div className="font-display text-2xl font-extrabold">{c.anonTitle}</div>
          <p className="mt-2 text-slate-600">{c.anonHint}</p>
          <button onClick={showLogin} className="mt-4 inline-flex rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop">{c.openLogin}</button>
        </div>
      ) : !activeId ? (
        <div className="grid gap-4">
          <div className="sticker-sm bg-white p-4">
            <div className="flex flex-wrap gap-2">
              {[
                { key: "all", label: c.view.all(viewCounts.all) },
                { key: "mine_to_reply", label: c.view.mine(viewCounts.mine_to_reply) },
                { key: "waiting_platform", label: c.view.platform(viewCounts.waiting_platform) },
                { key: "done", label: c.view.done(viewCounts.done) },
              ].map((item) => (
                <button
                  key={item.key}
                  type="button"
                  onClick={() => setViewFilter(item.key)}
                  className={`rounded-full border-2 border-slate-800 px-4 py-2 text-sm font-bold ${viewFilter === item.key ? "bg-violet-500 text-white" : "bg-white"}`}
                >
                  {item.label}
                </button>
              ))}
            </div>
            <div className="mt-3 grid gap-3 md:grid-cols-2">
              <label className="grid gap-1 text-sm font-bold">
                <span className="text-xs uppercase tracking-wider text-slate-500">{c.filterStatusLabel}</span>
                <select value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-white px-3 py-2">
                  <option value="all">{c.filterStatusOptions.all}</option>
                  <option value="open">{c.filterStatusOptions.open}</option>
                  <option value="waiting_admin">{c.filterStatusOptions.waitingAdmin}</option>
                  <option value="waiting_user">{c.filterStatusOptions.waitingUser}</option>
                  <option value="resolved">{c.filterStatusOptions.resolved}</option>
                  <option value="closed">{c.filterStatusOptions.closed}</option>
                </select>
              </label>
              <label className="grid gap-1 text-sm font-bold">
                <span className="text-xs uppercase tracking-wider text-slate-500">{c.filterPriorityLabel}</span>
                <select value={priorityFilter} onChange={(e) => setPriorityFilter(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-white px-3 py-2">
                  <option value="all">{c.priority.all}</option>
                  <option value="low">{c.priority.low}</option>
                  <option value="normal">{c.priority.normal}</option>
                  <option value="high">{c.priority.high}</option>
                  <option value="urgent">{c.priority.urgent}</option>
                </select>
              </label>
            </div>
          </div>
          <div className="grid gap-3">
            {tickets === null && (
              <>
                <Skeleton className="h-24 w-full rounded-2xl" />
                <Skeleton className="h-24 w-full rounded-2xl" />
              </>
            )}
            {tickets && filteredTickets.length === 0 && (
              <EmptyState shape="circle" title={c.noMatchTitle} hint={c.noMatchHint} />
            )}
            {tickets && filteredTickets.map((t) => {
              const typeKey = t.ticket_type as keyof typeof c.type | undefined;
              const typeLabel = typeKey && c.type[typeKey] ? c.type[typeKey] : t.ticket_type;
              const priorityKey = (t.priority ?? "normal") as keyof typeof c.priority;
              const priorityLabel = c.priority[priorityKey] ?? c.priority.normal;
              return (
                <button
                  key={String(t.id)}
                  onClick={() => setActiveId(t.id ?? null)}
                  className="text-left sticker bg-white p-5 lift"
                >
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="rounded-full border-2 border-slate-800 bg-amber-100 px-2 py-0.5 text-xs font-bold uppercase">{typeLabel}</span>
                      <Pill status={t.status ?? "open"} size="sm" />
                      <Pill variant={priorityVariant(t.priority)} size="sm">{priorityLabel}</Pill>
                    </div>
                    <div className="text-xs text-slate-500">{t.ticket_no} · {formatDate(t.created_at)}</div>
                  </div>
                  <div className="mt-3 font-display text-xl font-extrabold">{t.subject || c.emptySubject}</div>
                </button>
              );
            })}
          </div>
        </div>
      ) : (
        <div className="sticker bg-white p-6 min-h-[300px]">
          <TicketDetails id={activeId} onChanged={reload} onDeleted={() => { setActiveId(null); reload(); }} />
        </div>
      )}

      <CreateTicketModal open={createOpen} onClose={() => { setCreateOpen(false); reload(); }} />
    </div>
  );
}

function CreateTicketModal({ open, onClose }: { open: boolean; onClose: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].support;
  const [type, setType] = useState("feedback");
  const [subject, setSubject] = useState("");
  const [body, setBody] = useState("");
  const [priority, setPriority] = useState("normal");
  const [files, setFiles] = useState<File[]>([]);
  const [submitting, setSubmitting] = useState(false);

  useEffect(() => { if (open) { setType("feedback"); setSubject(""); setBody(""); setPriority("normal"); setFiles([]); } }, [open]);

  async function submit() {
    if (!subject.trim() || !body.trim()) { toast.push({ variant: "error", title: c.submitMissing }); return; }
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
      await apiPost("/v1/tickets", {
        ticket_type: type,
        subject: subject.trim(),
        body_text: body.trim(),
        priority,
        attachment_ids: attachmentIds
      });
      toast.push({ variant: "success", title: c.submitSuccess });
      onClose();
    } catch (err) {
      toast.push({ variant: "error", title: c.submitFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <Modal open={open} onClose={onClose} title={c.createTitle} description={c.createDesc} width="lg">
      <div className="grid gap-4">
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.categoryLabel}</span>
          <select value={type} onChange={(e) => setType(e.target.value)} className="rounded-2xl border-2 border-slate-800 bg-white px-4 py-3 font-bold">
            <option value="feedback">{c.category.feedback}</option>
            <option value="billing_issue">{c.category.billing}</option>
            <option value="account_issue">{c.category.account}</option>
          </select>
        </label>
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.priorityLabel}</span>
          <div className="flex flex-wrap gap-2">
            {(["low", "normal", "high"] as const).map((p) => (
              <button
                key={p}
                type="button"
                onClick={() => setPriority(p)}
                className={`rounded-full border-2 border-slate-800 px-4 py-2 text-sm font-bold ${priority === p ? "bg-violet-500 text-white" : "bg-white"}`}
              >
                {c.priority[p]}
              </button>
            ))}
          </div>
        </label>
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.subjectLabel}</span>
          <input value={subject} onChange={(e) => setSubject(e.target.value)} placeholder={c.subjectPlaceholder} className="rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
        </label>
        <label className="grid gap-2">
          <span className="text-xs font-bold uppercase tracking-wider text-slate-600">{c.bodyLabel}</span>
          <textarea value={body} onChange={(e) => setBody(e.target.value)} placeholder={c.bodyPlaceholder} className="min-h-32 rounded-2xl border-2 border-slate-800 bg-amber-50 px-4 py-3 outline-none focus:bg-white" />
        </label>
        <FilePicker files={files} onChange={setFiles} />
      </div>
      <ModalActions>
        <button onClick={onClose} className="mt-5 rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.cancel}</button>
        <button onClick={submit} disabled={submitting} className="mt-5 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop disabled:opacity-50">
          {submitting ? c.submitting : c.submit}
        </button>
      </ModalActions>
    </Modal>
  );
}

function TicketDetails({ id, onChanged, onDeleted }: { id: string; onChanged: () => void; onDeleted: () => void }) {
  const toast = useToast();
  const { locale } = useLocale();
  const c = copy[locale].support;
  const [details, setDetails] = useState<{ ticket?: Ticket; messages?: Message[]; attachments?: Attachment[] } | null>(null);
  const [reply, setReply] = useState("");
  const [files, setFiles] = useState<File[]>([]);
  const [submitting, setSubmitting] = useState(false);
  const [closeOpen, setCloseOpen] = useState(false);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const formatDate = useDateTimeFormatter();

  function reload() {
    apiGet<typeof details>(`/v1/tickets/${id}`).then(setDetails).catch(() => setDetails(null));
  }
  useEffect(() => { reload(); }, [id]);

  async function send() {
    if (!reply.trim()) { toast.push({ variant: "error", title: c.replyMissing }); return; }
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
      await apiPost(`/v1/tickets/${id}/messages`, { body_text: reply.trim(), attachment_ids: attachmentIds });
      setReply(""); setFiles([]);
      reload();
      onChanged();
    } catch (err) {
      toast.push({ variant: "error", title: c.replyFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setSubmitting(false);
    }
  }

  async function close() {
    try {
      await apiPost(`/v1/tickets/${id}/close`, {});
      toast.push({ variant: "success", title: c.closeSuccess });
      reload();
      onChanged();
    } catch (err) {
      toast.push({ variant: "error", title: c.closeFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setCloseOpen(false);
    }
  }

  async function remove() {
    try {
      await apiDelete(`/v1/tickets/${id}`);
      toast.push({ variant: "success", title: c.deleteSuccess });
      onDeleted();
    } catch (err) {
      toast.push({ variant: "error", title: c.deleteFailed, description: String(err).replace(/^Error:\s*/, "") });
    } finally {
      setDeleteOpen(false);
    }
  }

  if (!details) return <Skeleton className="h-40 w-full rounded-2xl" />;
  const ticket = details.ticket ?? {};
  const messages = details.messages ?? [];
  const attachments = details.attachments ?? [];
  const isClosed = ticket.status === "closed" || ticket.status === "resolved";
  const typeKey = ticket.ticket_type as keyof typeof c.type | undefined;
  const typeLabel = typeKey && c.type[typeKey] ? c.type[typeKey] : ticket.ticket_type;
  const priorityKey = (ticket.priority ?? "normal") as keyof typeof c.priority;
  const priorityLabel = c.priority[priorityKey] ?? c.priority.normal;
  const replyPlaceholder = locale === "zh"
    ? "补充说明、追问、上传截图…"
    : "Add context, ask follow-ups, drop screenshots…";
  const sendingLabel = locale === "zh" ? "发送中…" : "Sending…";
  const sendLabel = locale === "zh" ? "发送回复" : "Send reply";
  const closedNote = locale === "zh"
    ? "该工单已关闭，如有新问题请新建工单。"
    : "This ticket is closed. Open a new one if anything else comes up.";
  const closeDesc = locale === "zh"
    ? "关闭后无法继续回复，但记录会保留。"
    : "After closing, replies are blocked but the record stays.";
  const deleteDesc = locale === "zh"
    ? "该工单尚未被管理员回复，可彻底删除。删除后所有消息与附件不可恢复。"
    : "Tickets without admin replies can be deleted permanently. Messages and attachments cannot be recovered.";
  const internalNoteLabel = locale === "zh" ? "内部备注" : "Internal note";
  const noMessagesYet = locale === "zh" ? "暂无消息。" : "No messages yet.";

  return (
    <div className="grid gap-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <div className="text-xs uppercase tracking-wider text-slate-500">{typeLabel}</div>
          <h3 className="font-display text-2xl font-extrabold">{ticket.subject}</h3>
          <div className="text-xs text-slate-500">{ticket.ticket_no}</div>
        </div>
        <div className="flex flex-wrap items-center gap-2">
          <Pill status={ticket.status ?? "open"} />
          <Pill variant={priorityVariant(ticket.priority)}>{priorityLabel}</Pill>
          {ticket.can_close && (
            <button onClick={() => setCloseOpen(true)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-amber-200 px-3 py-1 text-xs font-bold lift"><XCircle size={12} /> {c.closeBtn}</button>
          )}
          {ticket.can_delete && (
            <button onClick={() => setDeleteOpen(true)} className="inline-flex items-center gap-1 rounded-full border-2 border-slate-800 bg-pink-200 px-3 py-1 text-xs font-bold lift"><Trash2 size={12} /> {c.deleteBtn}</button>
          )}
        </div>
      </div>

      <div className="relative ml-4 grid gap-3 border-l-2 border-dashed border-slate-300 pl-5">
        {messages.map((m, i) => {
          const messageAttachments = attachments.filter((attachment) => String((attachment as { message_id?: string }).message_id ?? "") === String(m.id ?? ""));
          return (
            <div key={String(m.id ?? i)} className="relative">
              <span className="absolute -left-7 top-2 h-3 w-3 rounded-full border-2 border-slate-800 bg-violet-300" />
              <div className={`rounded-2xl border-2 border-slate-800 p-3 ${m.sender_type === "admin" ? "bg-amber-50" : m.sender_type === "system" ? "bg-slate-50" : "bg-emerald-50"}`}>
                <div className="flex items-center gap-2 text-xs">
                  <b>{senderLabel(m.sender_type, locale)}</b>
                  {Boolean(m.internal_note) && <span className="rounded border border-dashed border-slate-500 px-1 text-slate-500">{internalNoteLabel}</span>}
                  <span className="text-slate-500">{formatDate(m.created_at)}</span>
                </div>
                <p className="mt-1 whitespace-pre-wrap text-sm">{m.body_text}</p>
                <div className="mt-3">
                  <ImageAttachmentGrid attachments={messageAttachments} />
                </div>
              </div>
            </div>
          );
        })}
        {messages.length === 0 && <div className="text-sm text-slate-500">{noMessagesYet}</div>}
      </div>

      {!isClosed ? (
        <div className="rounded-2xl border-2 border-slate-800 bg-white p-3">
          <textarea value={reply} onChange={(e) => setReply(e.target.value)} placeholder={replyPlaceholder} className="min-h-24 w-full rounded-xl bg-amber-50 p-2 outline-none focus:bg-white" />
          <div className="mt-2"><FilePicker files={files} onChange={setFiles} /></div>
          <div className="mt-3 flex justify-end">
            <button onClick={send} disabled={submitting} className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-4 py-2 font-bold text-white btn-pop disabled:opacity-50">
              <Send size={14} /> {submitting ? sendingLabel : sendLabel}
            </button>
          </div>
        </div>
      ) : (
        <div className="rounded-2xl border-2 border-slate-800 bg-slate-50 p-4 text-sm text-slate-600">{closedNote}</div>
      )}

      <Modal open={closeOpen} onClose={() => setCloseOpen(false)} title={c.closeConfirmTitle} description={closeDesc}>
        <ModalActions>
          <button onClick={() => setCloseOpen(false)} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.cancel}</button>
          <button onClick={close} className="rounded-full border-2 border-slate-800 bg-amber-300 px-5 py-2 font-bold btn-pop">{c.closeOk}</button>
        </ModalActions>
      </Modal>

      <Modal open={deleteOpen} onClose={() => setDeleteOpen(false)} title={c.deleteConfirmTitle} description={deleteDesc}>
        <ModalActions>
          <button onClick={() => setDeleteOpen(false)} className="rounded-full border-2 border-slate-800 bg-white px-5 py-2 font-bold lift">{c.cancel}</button>
          <button onClick={remove} className="rounded-full border-2 border-slate-800 bg-pink-400 px-5 py-2 font-bold text-white btn-pop">{c.deleteOk}</button>
        </ModalActions>
      </Modal>
    </div>
  );
}

function senderLabel(t: string | undefined, locale: "zh" | "en") {
  if (locale === "en") {
    switch (t) {
      case "admin": return "Admin";
      case "user": return "User";
      case "provider": return "Provider";
      case "system": return "System";
      default: return t || "Message";
    }
  }
  switch (t) {
    case "admin": return "管理员";
    case "user": return "用户";
    case "provider": return "Provider";
    case "system": return "系统";
    default: return t || "消息";
  }
}
