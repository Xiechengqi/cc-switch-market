export type SiteAnnouncement = {
  id: string;
  type: "system" | "billing" | "pricing" | "maintenance";
  title: { zh: string; en: string };
  body: { zh: string; en: string };
  publishedAt: string;
  pinned?: boolean;
  active?: boolean;
};

export const ANNOUNCEMENTS_STORAGE_KEY = "cc_switch_market_announcements";
export const ANNOUNCEMENTS_SEEN_KEY = "cc_switch_market_seen_announcements";

export const defaultAnnouncements: SiteAnnouncement[] = [
  {
    id: "router-email-auth-live",
    type: "system",
    title: {
      zh: "邮箱验证码登录已启用",
      en: "Email code login is live"
    },
    body: {
      zh: "cc-switch Market 现已统一使用 router 邮箱验证码登录。Provider 请使用 share owner email 登录后领取收益。",
      en: "cc-switch Market now uses router email code login. Providers should sign in with the share owner email to claim earnings."
    },
    publishedAt: "2026-04-29T10:00:00Z",
    pinned: true,
    active: true
  },
  {
    id: "pricing-on-homepage",
    type: "pricing",
    title: {
      zh: "价格表已合并到首页",
      en: "Pricing moved to the homepage"
    },
    body: {
      zh: "统一 token 价格、平台抽成说明与接入示例，现已集中展示在首页，减少路由跳转。",
      en: "Unified token pricing, the platform take explanation, and quick-start examples are now grouped on the homepage."
    },
    publishedAt: "2026-04-29T12:00:00Z",
    pinned: false,
    active: true
  },
  {
    id: "payout-review-note",
    type: "maintenance",
    title: {
      zh: "提现失败将进入待复核",
      en: "Failed payouts enter review"
    },
    body: {
      zh: "Gate.io 网络异常或结果未知时，提现请求会进入“待复核”，避免重复打款。",
      en: "When Gate.io times out or returns an uncertain result, payout requests move to review to avoid duplicate transfers."
    },
    publishedAt: "2026-04-29T14:00:00Z",
    pinned: false,
    active: true
  }
];

export function readAnnouncements(): SiteAnnouncement[] {
  if (typeof window === "undefined") return defaultAnnouncements;
  try {
    const raw = window.localStorage.getItem(ANNOUNCEMENTS_STORAGE_KEY);
    if (!raw) return defaultAnnouncements;
    const parsed = JSON.parse(raw);
    if (!Array.isArray(parsed)) return defaultAnnouncements;
    return parsed as SiteAnnouncement[];
  } catch {
    return defaultAnnouncements;
  }
}

export function writeAnnouncements(items: SiteAnnouncement[]) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(ANNOUNCEMENTS_STORAGE_KEY, JSON.stringify(items));
  window.dispatchEvent(new CustomEvent("cc-switch-market:announcements-updated"));
}

export function readSeenAnnouncementIds(): string[] {
  if (typeof window === "undefined") return [];
  try {
    const raw = window.localStorage.getItem(ANNOUNCEMENTS_SEEN_KEY);
    if (!raw) return [];
    const parsed = JSON.parse(raw);
    return Array.isArray(parsed) ? parsed.filter((v) => typeof v === "string") : [];
  } catch {
    return [];
  }
}

export function writeSeenAnnouncementIds(ids: string[]) {
  if (typeof window === "undefined") return;
  window.localStorage.setItem(ANNOUNCEMENTS_SEEN_KEY, JSON.stringify(ids));
  window.dispatchEvent(new CustomEvent("cc-switch-market:announcements-seen-updated"));
}

export function markAllAnnouncementsRead(items: SiteAnnouncement[]) {
  writeSeenAnnouncementIds(items.filter((item) => item.active !== false).map((item) => item.id));
}
