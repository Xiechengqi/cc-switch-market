"use client";

import { useEffect, useMemo, useState } from "react";
import { Bell } from "lucide-react";
import { AnnouncementModal } from "@/components/ui/AnnouncementModal";
import { readAnnouncements, readSeenAnnouncementIds } from "@/lib/site-announcements";

export function AnnouncementButton({ iconOnly = false }: { iconOnly?: boolean }) {
  const [open, setOpen] = useState(false);
  const [items, setItems] = useState(readAnnouncements);
  const [seen, setSeen] = useState(readSeenAnnouncementIds);

  useEffect(() => {
    function refresh() {
      setItems(readAnnouncements());
      setSeen(readSeenAnnouncementIds());
    }
    refresh();
    window.addEventListener("cc-switch-market:announcements-updated", refresh as EventListener);
    window.addEventListener("cc-switch-market:announcements-seen-updated", refresh as EventListener);
    window.addEventListener("storage", refresh);
    return () => {
      window.removeEventListener("cc-switch-market:announcements-updated", refresh as EventListener);
      window.removeEventListener("cc-switch-market:announcements-seen-updated", refresh as EventListener);
      window.removeEventListener("storage", refresh);
    };
  }, []);

  const unreadCount = useMemo(() => {
    const activeIds = items.filter((item) => item.active !== false).map((item) => item.id);
    return activeIds.filter((id) => !seen.includes(id)).length;
  }, [items, seen]);

  return (
    <>
      <button
        type="button"
        onClick={() => setOpen(true)}
        aria-label="系统公告"
        title="系统公告"
        className={`relative inline-flex items-center justify-center rounded-full border-2 border-[var(--border)] bg-[var(--card)] font-bold text-[var(--foreground)] lift ${iconOnly ? "h-10 w-10" : "px-3 py-2 text-sm"}`}
      >
        <Bell size={16} />
        {unreadCount > 0 && (
          <span className="absolute -right-1 -top-1 inline-flex min-w-5 items-center justify-center rounded-full border-2 border-[var(--border)] bg-pink-400 px-1 text-[10px] font-extrabold text-white">
            {unreadCount}
          </span>
        )}
      </button>
      <AnnouncementModal open={open} onClose={() => setOpen(false)} onReadAll={setSeen} />
    </>
  );
}
