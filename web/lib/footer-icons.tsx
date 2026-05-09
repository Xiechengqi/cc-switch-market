import { Activity, BookOpen, Github, Globe, Link as LinkIcon, ScrollText, Twitter, type LucideIcon } from "lucide-react";

export const FOOTER_ICON_KEYS = ["link", "twitter", "github", "globe", "book", "activity", "scroll"] as const;

export type FooterIconKey = (typeof FOOTER_ICON_KEYS)[number];

const ICON_MAP: Record<FooterIconKey, LucideIcon> = {
  link: LinkIcon,
  twitter: Twitter,
  github: Github,
  globe: Globe,
  book: BookOpen,
  activity: Activity,
  scroll: ScrollText,
};

export function resolveFooterIcon(key: string): LucideIcon {
  return ICON_MAP[(FOOTER_ICON_KEYS as readonly string[]).includes(key) ? (key as FooterIconKey) : "link"];
}

export const FOOTER_ICON_LABELS: Record<FooterIconKey, string> = {
  link: "Link",
  twitter: "X / Twitter",
  github: "GitHub",
  globe: "Globe",
  book: "Book",
  activity: "Activity",
  scroll: "Scroll",
};
