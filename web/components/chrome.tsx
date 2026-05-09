"use client";

import Link from "next/link";
import { Menu, X } from "lucide-react";
import { useState, type ReactNode } from "react";
import { AuthSlot, useMarketAuth } from "@/components/auth";
import { useLocale } from "@/components/language-provider";
import { MarketLogo } from "@/components/ui/Logo";
import { AnnouncementButton } from "@/components/ui/AnnouncementButton";
import { LanguageSwitch } from "@/components/ui/LanguageSwitch";
import { copy, BRAND } from "@/lib/copy";
import { usePublicConfig, type FooterLink } from "@/lib/public-config";
import { resolveFooterIcon } from "@/lib/footer-icons";

type NavItem = { href: string; label: string; admin?: boolean };

export function Shell({ children }: { children: React.ReactNode }) {
  const { user } = useMarketAuth();
  const { t } = useLocale();
  const [open, setOpen] = useState(false);
  const items: NavItem[] = [
    { href: "/", label: t.nav.home },
    { href: "/dashboard", label: t.nav.dashboard },
    { href: "/claim", label: t.nav.claim },
    { href: "/support", label: t.nav.support },
    { href: "/board", label: t.nav.board },
    { href: "/admin", label: t.nav.admin, admin: true }
  ].filter((item) => !item.admin || user?.isAdmin);

  return (
    <main className="relative min-h-screen overflow-hidden text-fg">
      <Decorations />
      <nav className="relative z-30 mx-auto flex max-w-6xl items-center justify-between gap-3 px-5 py-5 md:px-6 md:py-6">
        <Link href="/" aria-label="cc-switch Market 首页" className="inline-flex items-center gap-3">
          <MarketLogo size={40} className="animate-wiggle" withConfetti={false} />
          <span className="hidden font-display text-xl font-extrabold tracking-tight md:inline">cc-switch Market</span>
        </Link>
        <div className="hidden items-center gap-2 md:flex">
          {items.map((item) => (
            <NavLink key={item.href} href={item.href}>{item.label}</NavLink>
          ))}
        </div>
        <div className="hidden items-center gap-2 md:flex">
          <AnnouncementButton iconOnly />
          <LanguageSwitch />
          <AuthSlot />
        </div>
        <div className="flex items-center gap-2 md:hidden">
          <button
            type="button"
            aria-label={t.nav.menu}
            onClick={() => setOpen(true)}
            className="rounded-full border-2 border-[var(--border)] bg-[var(--card)] p-2 text-fg"
          >
            <Menu size={18} />
          </button>
        </div>
      </nav>
      <div className="relative z-10">{children}</div>
      <SiteFooter />
      {open && <MobileMenu items={items} onClose={() => setOpen(false)} />}
    </main>
  );
}

function SiteFooter() {
  const { locale } = useLocale();
  const f = copy[locale].footer;
  const brand = BRAND[locale];
  const config = usePublicConfig();
  const links: FooterLink[] = config.footerLinks ?? [];
  const linkCls = "inline-flex items-center gap-2 text-sm font-bold text-slate-700 transition-colors duration-200 hover:text-violet-600 motion-safe:hover:translate-x-0.5";
  return (
    <footer className="relative z-10 mt-12">
      <div className="mx-auto grid max-w-6xl gap-8 px-5 py-10 md:grid-cols-[1.4fr_minmax(0,1fr)] md:px-6">
        <div>
          <div className="inline-flex items-center gap-3">
            <MarketLogo size={36} withConfetti={false} />
            <span className="font-display text-lg font-extrabold tracking-tight">{brand.name}</span>
          </div>
          <p className="mt-3 max-w-sm text-sm text-slate-600">{f.tagline}</p>
          <p className="mt-2 max-w-sm text-xs text-slate-500">{f.builtWith}</p>
        </div>
        {links.length > 0 && (
          <ul
            className="flex flex-col gap-2 md:grid md:gap-x-10 md:gap-y-2"
            style={{
              gridTemplateRows: "repeat(3, auto)",
              gridAutoFlow: "column",
              gridAutoColumns: "max-content",
            }}
          >
            {links.map((item, index) => {
              const label = (locale === "zh" ? item.labelZh : item.labelEn) || item.labelEn || item.labelZh;
              if (!label) return null;
              const Icon = resolveFooterIcon(item.icon);
              const url = item.url || "#";
              const isExternal = /^https?:\/\//.test(url);
              return (
                <li key={`${url}-${index}`}>
                  <a
                    href={url}
                    className={linkCls}
                    aria-label={label}
                    target={isExternal ? "_blank" : undefined}
                    rel={isExternal ? "noopener noreferrer" : undefined}
                  >
                    <Icon size={14} />
                    <span>{label}</span>
                  </a>
                </li>
              );
            })}
          </ul>
        )}
      </div>
      <div className="mx-auto max-w-6xl px-5 pb-8 text-center text-xs font-bold text-slate-500 md:px-6">
        {f.legal}
      </div>
    </footer>
  );
}

function MobileMenu({ items, onClose }: { items: NavItem[]; onClose: () => void }) {
  const { t } = useLocale();
  return (
    <div className="fixed inset-0 z-50 flex bg-slate-900/40 md:hidden" onClick={onClose}>
      <div className="ml-auto flex h-full w-80 max-w-[90vw] flex-col gap-4 border-l-2 border-[var(--border)] bg-[var(--card)] p-6 text-fg" onClick={(e) => e.stopPropagation()}>
        <div className="flex items-center justify-between">
          <span className="font-display text-xl font-extrabold">{t.nav.menu}</span>
          <button onClick={onClose} aria-label={t.common.close} className="rounded-full border-2 border-[var(--border)] bg-[var(--card)] p-1.5">
            <X size={16} />
          </button>
        </div>
        <div className="grid gap-3 rounded-2xl border-2 border-[var(--border)] bg-[var(--card)] p-3">
          <div className="flex items-center justify-between gap-3">
            <span className="text-sm font-bold text-[var(--muted-foreground)]">{t.nav.announcements}</span>
            <AnnouncementButton iconOnly />
          </div>
          <div className="flex items-center justify-between gap-3">
            <span className="text-sm font-bold text-[var(--muted-foreground)]">{t.common.language}</span>
            <LanguageSwitch />
          </div>
        </div>
        <div className="flex flex-col gap-2">
          {items.map((item) => (
            <Link key={item.href} href={item.href} onClick={onClose} className="rounded-full border-2 border-[var(--border)] bg-amber-100 px-4 py-3 font-bold lift text-fg">
              {item.label}
            </Link>
          ))}
        </div>
        <div className="mt-auto"><AuthSlot /></div>
      </div>
    </div>
  );
}

export function Card({ title, children, color = "bg-[var(--card)]" }: { title: string; children: ReactNode; color?: string }) {
  return (
    <section className={`sticker ${color} p-6 lift text-fg`}>
      <h2 className="font-display text-2xl font-extrabold">{title}</h2>
      <div className="mt-3 text-[var(--muted-foreground)]">{children}</div>
    </section>
  );
}

function NavLink({ href, children }: { href: string; children: ReactNode }) {
  return (
    <Link href={href} className="rounded-full px-4 py-2 font-bold hover:bg-amber-300 text-fg">
      {children}
    </Link>
  );
}

function Decorations() {
  return (
    <div aria-hidden className="pointer-events-none fixed inset-0 overflow-hidden">
      <div className="absolute -left-20 top-32 h-56 w-56 rounded-full bg-amber-300/70" />
      <div className="absolute right-10 top-24 h-24 w-24 rotate-12 rounded-3xl bg-pink-400/70 hidden md:block" />
      <div className="absolute right-[-40px] bottom-1/4 h-40 w-40 rotate-12 bg-violet-300/50 hidden lg:block" style={{ borderRadius: "60% 40% 50% 60% / 50% 60% 40% 50%" }} />
    </div>
  );
}
