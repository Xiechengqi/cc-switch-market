"use client";

import { useEffect, useState } from "react";
import { useLocale } from "@/components/language-provider";
import { Modal } from "@/components/ui/Modal";
import { Pill } from "@/components/ui/Pill";
import { EmptyState } from "@/components/ui/EmptyState";
import {
  type SiteAnnouncement,
  readAnnouncements,
  readSeenAnnouncementIds,
  markAllAnnouncementsRead
} from "@/lib/site-announcements";
import { useDateTimeFormatter } from "@/lib/time";

function typeLabel(type: SiteAnnouncement["type"], locale: "zh" | "en") {
  const map = {
    system: { zh: "系统", en: "System" },
    billing: { zh: "支付", en: "Billing" },
    pricing: { zh: "价格", en: "Pricing" },
    maintenance: { zh: "维护", en: "Maintenance" }
  } as const;
  return map[type][locale];
}

export function AnnouncementModal({
  open,
  onClose,
  onReadAll
}: {
  open: boolean;
  onClose: () => void;
  onReadAll?: (seenIds: string[]) => void;
}) {
  const { locale, t } = useLocale();
  const [items, setItems] = useState<SiteAnnouncement[]>(readAnnouncements);
  const [seen, setSeen] = useState<string[]>(readSeenAnnouncementIds);
  const formatDate = useDateTimeFormatter();

  useEffect(() => {
    function refresh() {
      setItems(readAnnouncements());
      setSeen(readSeenAnnouncementIds());
    }
    window.addEventListener("cc-switch-market:announcements-updated", refresh as EventListener);
    window.addEventListener("cc-switch-market:announcements-seen-updated", refresh as EventListener);
    window.addEventListener("storage", refresh);
    return () => {
      window.removeEventListener("cc-switch-market:announcements-updated", refresh as EventListener);
      window.removeEventListener("cc-switch-market:announcements-seen-updated", refresh as EventListener);
      window.removeEventListener("storage", refresh);
    };
  }, []);

  const activeItems = items.filter((item) => item.active !== false);

  function handleMarkAllRead() {
    const next = activeItems.map((item) => item.id);
    markAllAnnouncementsRead(activeItems);
    setSeen(next);
    onReadAll?.(next);
  }

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t.nav.announcements}
      description={locale === "zh" ? "最近更新、价格提醒与维护通知。" : "Updates, pricing notes, and maintenance windows."}
      width="lg"
    >
      <div className="grid gap-4">
        <div className="flex items-center justify-end">
          <button onClick={handleMarkAllRead} className="rounded-full border-2 border-[var(--border)] bg-amber-100 px-3 py-1 text-xs font-bold lift text-fg">
            {locale === "zh" ? "全部已读" : "Mark all read"}
          </button>
        </div>
        {activeItems.length === 0 ? (
          <EmptyState shape="circle" title={locale === "zh" ? "暂时没有公告" : "No announcements"} />
        ) : (
          <div className="grid gap-3">
            {activeItems.map((item) => {
              const unread = !seen.includes(item.id);
              return (
                <div key={item.id} className="rounded-2xl border-2 border-[var(--border)] bg-[var(--card)] p-4 text-[var(--foreground)]">
                  <div className="flex flex-wrap items-center justify-between gap-3">
                    <div className="flex items-center gap-2">
                      <Pill variant={item.pinned ? "warning" : "info"}>{typeLabel(item.type, locale)}</Pill>
                      {item.pinned && <Pill variant="warning">{locale === "zh" ? "置顶" : "Pinned"}</Pill>}
                      {unread && <Pill variant="review">{locale === "zh" ? "未读" : "Unread"}</Pill>}
                    </div>
                    <span className="text-xs text-[var(--muted-foreground)]">{formatDate(item.publishedAt)}</span>
                  </div>
                  <h3 className="mt-3 font-display text-xl font-extrabold">{item.title[locale]}</h3>
                  <p className="mt-2 text-sm leading-6 text-[var(--muted-foreground)]">{item.body[locale]}</p>
                </div>
              );
            })}
          </div>
        )}
      </div>
    </Modal>
  );
}
