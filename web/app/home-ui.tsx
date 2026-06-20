"use client";

import Link from "next/link";
import { useEffect, useMemo, useState } from "react";
import {
  ArrowRight,
  Coins,
  ReceiptText,
  LifeBuoy,
  ShieldCheck,
  Zap,
  Sparkles,
  Star,
  KeyRound,
  TerminalSquare,
  PlugZap,
  Store,
  Users,
  Copy as CopyIcon,
  Check,
  ExternalLink
} from "lucide-react";
import { apiGet } from "@/lib/client-api";
import { Confetti } from "@/components/ui/Confetti";
import { Skeleton } from "@/components/ui/Skeleton";
import { MarketLogo, RouterLogo } from "@/components/ui/Logo";
import { useLocale } from "@/components/language-provider";
import { useToast } from "@/components/ui/Toast";
import { copy } from "@/lib/copy";
import { commissionText, usePublicConfig } from "@/lib/public-config";

type Price = {
  id: string;
  app_type: string;
  model_pattern: string;
  input_per_million: string;
  output_per_million: string;
  cache_read_per_million?: string | null;
  cache_write_per_million?: string | null;
  official_input_per_million?: string | null;
  official_output_per_million?: string | null;
  official_cache_read_per_million?: string | null;
  official_cache_write_per_million?: string | null;
  discount_percent?: string | number | null;
  status?: string;
};

const DEFAULT_PRICE_PROVIDERS = ["anthropic", "openai", "gemini", "deepseek"];
const PRICE_PAGE_SIZE = 6;

export function HomeHero() {
  const { locale } = useLocale();
  const c = copy[locale].landing;
  const publicConfig = usePublicConfig();
  const commission = commissionText(locale, publicConfig.totalCommissionBps);
  return (
    <section className="relative mx-auto grid max-w-6xl gap-10 px-6 py-16 md:py-24 lg:grid-cols-[1.1fr_0.9fr] lg:items-center">
      <Confetti density="medium" />
      <div className="relative z-10">
        <MarketLogo size={72} className="mb-5 animate-wiggle" />
        <div className="mb-5 inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-4 py-2 font-bold">
          <Sparkles size={16} /> {commission.heroBadge}
        </div>
        <div className="font-display text-2xl font-extrabold uppercase tracking-wider text-violet-600 md:text-3xl">
          {c.heroBrand}
        </div>
        <h1 className="mt-2 font-display text-5xl font-extrabold leading-[1.05] md:text-7xl">
          {c.heroH1Prefix}
          <br />
          <span className="bg-amber-300 px-2 -rotate-1 inline-block border-2 border-slate-800 rounded-2xl">
            {c.heroH1Highlight}
          </span>
        </h1>
        <p className="mt-6 max-w-xl text-lg leading-7 text-slate-700">{c.heroSubtitle}</p>
        <div className="mt-5 inline-flex max-w-xl items-center gap-2 rounded-2xl border-2 border-slate-800 bg-amber-300 px-4 py-3 font-display text-xl font-extrabold text-slate-900">
          <Star size={20} />
          <span>{c.heroPricePromise}</span>
        </div>
        <div className="mt-8 flex flex-wrap gap-4">
          <Link
            href="/dashboard"
            className="inline-flex items-center gap-3 rounded-full border-2 border-slate-800 bg-violet-500 px-6 py-3 font-bold text-white btn-pop"
          >
            {c.heroCtaPrimary} <ArrowRight className="rounded-full bg-white p-1 text-violet-500" size={28} />
          </Link>
          <Link
            href="/claim"
            className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-white px-6 py-3 font-bold btn-pop hover:bg-amber-300"
          >
            {c.heroCtaSecondary}
          </Link>
          <a href="#pricing" className="inline-flex items-center px-3 py-3 font-bold underline-offset-4 hover:underline">
            {c.heroPricingLink}
          </a>
        </div>
      </div>
      <div className="relative z-10 sticker bg-white p-6 rotate-2 animate-pop-in">
        <div className="grid gap-4">
          <Metric icon={<Coins size={20} />} label={c.metrics[0].label} value={c.metrics[0].value} color="bg-emerald-400 text-white" />
          <Metric icon={<ReceiptText size={20} />} label={c.metrics[1].label} value={c.metrics[1].value} color="bg-pink-400 text-white" />
          <Metric icon={<LifeBuoy size={20} />} label={c.metrics[2].label} value={c.metrics[2].value} color="bg-amber-300" />
        </div>
      </div>
    </section>
  );
}

function Metric({ icon, label, value, color }: { icon: React.ReactNode; label: string; value: string; color: string }) {
  return (
    <div className="flex items-center gap-4 rounded-2xl border-2 border-slate-800 bg-white p-4">
      <div className={`rounded-full border-2 border-slate-800 ${color} p-3`}>{icon}</div>
      <div className="flex-1">
        <div className="text-xs font-bold uppercase tracking-wider text-slate-500">{label}</div>
        <div className="font-display text-xl font-extrabold">{value}</div>
      </div>
    </div>
  );
}

export function MoneyFlow() {
  const { locale } = useLocale();
  const c = copy[locale].landing;
  const publicConfig = usePublicConfig();
  type Step = { label: string; note: string; color: string; icon?: React.ReactNode; href?: string };
  const palette = ["bg-violet-100", "bg-amber-100", "bg-pink-100", "bg-emerald-100", "bg-violet-100"];
  const customIcons: Record<number, React.ReactNode> = {
    1: <MarketLogo size={28} withConfetti={false} />,
    2: <RouterLogo size={28} />,
  };
  const customHrefs: Record<number, string | undefined> = {
    2: publicConfig.routerApiBaseUrl || undefined,
    3: "https://tokenswitch.org",
  };
  const steps: Step[] = c.flowSteps.map((s, i) => ({
    label: s.label,
    note: s.note,
    color: palette[i] ?? "bg-violet-100",
    icon: customIcons[i],
    href: customHrefs[i],
  }));
  return (
    <section className="relative mx-auto max-w-6xl bg-dot-grid-soft px-6 py-12">
      <h2 className="font-display text-3xl font-extrabold md:text-4xl">{c.flowTitle}</h2>
      <p className="mt-2 text-slate-600">{c.flowSubtitle}</p>
      <div className="mt-8 grid gap-3 lg:grid-cols-5">
        {steps.map((s, i) => {
          const cardBody = (
            <>
              <div className="flex items-center justify-between gap-2">
                <div className="text-xs font-bold uppercase tracking-wider text-slate-500">{c.flowStepBadge(i + 1)}</div>
                <div className="flex items-center gap-1.5">
                  {s.icon}
                  {s.href && <ExternalLink size={14} className="text-slate-500 group-hover:text-violet-600" aria-hidden />}
                </div>
              </div>
              <div className="mt-1 font-display text-lg font-extrabold">{s.label}</div>
              <div className="mt-1 text-xs text-slate-600">{s.note}</div>
            </>
          );
          return (
            <div key={s.label} className="relative">
              {s.href ? (
                <a
                  href={s.href}
                  target="_blank"
                  rel="noopener noreferrer"
                  aria-label={`${s.label} ↗`}
                  className={`group block sticker-sm ${s.color} h-full p-4 lift focus:outline-none focus-visible:ring-2 focus-visible:ring-violet-500 hover:border-violet-500`}
                >
                  {cardBody}
                </a>
              ) : (
                <div className={`sticker-sm ${s.color} h-full p-4 lift`}>{cardBody}</div>
              )}
              {i < steps.length - 1 && (
                <>
                  <div className="absolute right-[-12px] top-1/2 hidden -translate-y-1/2 lg:block" aria-hidden>
                    <ArrowRight size={18} className="text-slate-700" />
                  </div>
                  <div className="absolute left-1/2 -bottom-3 -translate-x-1/2 lg:hidden" aria-hidden>
                    <ArrowRight size={18} className="rotate-90 text-slate-700" />
                  </div>
                </>
              )}
            </div>
          );
        })}
      </div>
    </section>
  );
}

export function FeatureGrid() {
  const { locale } = useLocale();
  const c = copy[locale].landing;
  const publicConfig = usePublicConfig();
  const commission = commissionText(locale, publicConfig.totalCommissionBps);
  const icons = [
    <ReceiptText key="r" size={20} />,
    <Coins key="c" size={28} />,
    <Zap key="z" size={20} />,
    <ShieldCheck key="s" size={20} />,
    <PlugZap key="p" size={20} />,
    <LifeBuoy key="l" size={20} />,
  ];
  const colors = [
    "bg-violet-400 text-white",
    "bg-amber-300",
    "bg-pink-400 text-white",
    "bg-emerald-400 text-white",
    "bg-violet-400 text-white",
    "bg-pink-400 text-white",
  ];
  return (
    <section className="mx-auto max-w-6xl px-6 py-12">
      <h2 className="font-display text-3xl font-extrabold md:text-4xl">{c.featuresTitle}</h2>
      <div className="mt-6 grid auto-rows-fr gap-5 sm:grid-cols-2 md:grid-cols-3">
        {c.features.map((it, idx) => {
          if (idx === 1) {
            return (
              <article
                key={it.title}
                className="relative sticker bg-amber-200 p-8 lift sm:col-span-2 md:row-span-2 overflow-hidden"
              >
                <span aria-hidden className="pointer-events-none absolute -right-10 -top-10 h-40 w-40 rounded-full bg-amber-300/70" />
                <span aria-hidden className="pointer-events-none absolute -left-6 bottom-6 h-20 w-20 rotate-12 rounded-3xl border-2 border-slate-800 bg-pink-200" />
                <div className={`relative z-10 inline-flex items-center gap-3 rounded-full border-2 border-slate-800 px-3 py-2 ${colors[idx]} animate-wiggle`}>
                  {icons[idx]}
                  <span className="font-display text-base font-extrabold uppercase tracking-wider text-slate-900">{commission.featureTitle}</span>
                </div>
                <div className="relative z-10 mt-6 font-display text-6xl font-extrabold leading-none text-slate-900 md:text-7xl">
                  {commission.rate}
                </div>
                <p className="relative z-10 mt-4 max-w-md text-base leading-7 text-slate-800">
                  {commission.featureBody}
                </p>
              </article>
            );
          }
          return (
            <article key={it.title} className="relative sticker bg-white p-6 lift">
              <div className={`absolute -top-4 -left-3 rounded-full border-2 border-slate-800 p-2 animate-wiggle ${colors[idx]}`}>{icons[idx]}</div>
              <h3 className="mt-3 font-display text-xl font-extrabold">{it.title}</h3>
              <p className="mt-2 text-sm text-slate-600">{it.body}</p>
            </article>
          );
        })}
      </div>
    </section>
  );
}

export function PricingTable() {
  const { locale } = useLocale();
  const c = copy[locale].landing;
  const publicConfig = usePublicConfig();
  const [prices, setPrices] = useState<Price[] | null>(null);
  const [activeProvider, setActiveProvider] = useState("anthropic");
  const [page, setPage] = useState(0);
  useEffect(() => {
    apiGet<Price[]>("/v1/prices").then(setPrices).catch(() => setPrices([]));
  }, []);
  const priceProviders = useMemo(() => priceProviderTabs(prices), [prices]);
  useEffect(() => {
    if (!prices || priceProviders.length === 0) return;
    if (!priceProviders.includes(activeProvider)) setActiveProvider(priceProviders[0]);
  }, [prices, priceProviders, activeProvider]);
  useEffect(() => {
    setPage(0);
  }, [activeProvider]);

  const providerPrices = (prices ?? [])
    .filter((price) => price.app_type === activeProvider)
    .filter((price) => price.model_pattern !== "*")
    .sort(comparePriceDesc);
  const totalPages = Math.max(1, Math.ceil(providerPrices.length / PRICE_PAGE_SIZE));
  const currentPage = Math.min(page, totalPages - 1);
  const pagePrices = providerPrices.slice(
    currentPage * PRICE_PAGE_SIZE,
    currentPage * PRICE_PAGE_SIZE + PRICE_PAGE_SIZE
  );
  const activeDiscount = providerPrices.find((price) => price.discount_percent !== undefined)?.discount_percent ?? 10;
  const totalTake = commissionText(locale, publicConfig.totalCommissionBps).rate;
  const pricingBadge = locale === "zh"
    ? `官方价 ${formatDiscountPercent(activeDiscount)} · ${totalTake} 总抽成`
    : `${formatDiscountPercent(activeDiscount)} official price · ${totalTake} total take`;

  const cols = c.pricingCol;

  return (
    <section id="pricing" className="relative mx-auto max-w-6xl scroll-mt-20 bg-stripes-soft px-6 py-16">
      <h2 className="font-display text-4xl font-extrabold md:text-5xl">{c.pricingTitle}</h2>
      <div className="mt-5 flex flex-wrap items-center justify-between gap-3">
        <div className="flex flex-wrap gap-2">
          {priceProviders.map((provider) => {
            const count = (prices ?? []).filter((price) => price.app_type === provider && price.model_pattern !== "*").length;
            return (
              <button
                key={provider}
                type="button"
                onClick={() => setActiveProvider(provider)}
                className={`rounded-full border-2 border-slate-800 px-4 py-2 text-sm font-extrabold uppercase lift ${
                  activeProvider === provider ? "bg-violet-500 text-white" : "bg-white"
                }`}
              >
                {provider}
                {prices ? <span className="ml-2 rounded-full bg-amber-300 px-2 py-0.5 text-xs text-slate-900">{count}</span> : null}
              </button>
            );
          })}
        </div>
        <div className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-amber-300 px-3 py-1.5 text-sm font-extrabold text-slate-900">
          <Star size={14} />
          <span>{pricingBadge}</span>
        </div>
      </div>

      {prices === null && (
        <div className="mt-8 grid gap-3">
          <Skeleton className="h-12 w-full rounded-2xl" />
          <Skeleton className="h-12 w-full rounded-2xl" />
          <Skeleton className="h-12 w-full rounded-2xl" />
        </div>
      )}

      {prices && prices.length === 0 && (
        <div className="mt-8 sticker bg-amber-50 p-8 text-center">
          <div className="font-display text-xl font-extrabold">{c.pricingEmpty.title}</div>
          <p className="mt-1 text-sm text-slate-600">{c.pricingEmpty.body}</p>
        </div>
      )}

      {prices && prices.length > 0 && (
        <div className="mt-8">
          {providerPrices.length === 0 ? (
            <div className="sticker bg-amber-50 p-8 text-center">
              <div className="font-display text-xl font-extrabold">{c.pricingProviderEmpty.title(activeProvider)}</div>
              <p className="mt-1 text-sm text-slate-600">{c.pricingProviderEmpty.body}</p>
            </div>
          ) : (
            <div className="overflow-hidden rounded-3xl border-2 border-slate-800 bg-white">
              <table className="hidden w-full md:table">
                <thead className="bg-amber-100">
                  <tr>
                    <Th>{cols.model}</Th>
                    <Th align="right">{cols.input}</Th>
                    <Th align="right">{cols.output}</Th>
                    <Th align="right">{cols.cacheRead}</Th>
                    <Th align="right">{cols.cacheWrite}</Th>
                  </tr>
                </thead>
                <tbody>
                  {pagePrices.map((p, i) => (
                    <tr key={p.id} className={`border-t-2 border-slate-200 ${i % 2 === 1 ? "bg-amber-50/40" : ""}`}>
                      <td className="px-4 py-3 font-mono text-sm">{p.model_pattern}</td>
                      <td className="px-4 py-3 text-right font-mono"><PriceValue value={p.input_per_million} officialValue={p.official_input_per_million} /></td>
                      <td className="px-4 py-3 text-right font-mono"><PriceValue value={p.output_per_million} officialValue={p.official_output_per_million} /></td>
                      <td className="px-4 py-3 text-right font-mono"><PriceValue value={p.cache_read_per_million} officialValue={p.official_cache_read_per_million} /></td>
                      <td className="px-4 py-3 text-right font-mono"><PriceValue value={p.cache_write_per_million} officialValue={p.official_cache_write_per_million} /></td>
                    </tr>
                  ))}
                </tbody>
              </table>
              <div className="grid gap-3 p-3 md:hidden">
                {pagePrices.map((p) => (
                  <div key={p.id} className="rounded-2xl border-2 border-slate-800 bg-white p-3">
                    <div className="flex items-center justify-between">
                      <span className="rounded-full bg-violet-100 border-2 border-slate-800 px-2 py-0.5 text-xs font-bold uppercase">{activeProvider}</span>
                      <span className="font-mono text-sm">{p.model_pattern}</span>
                    </div>
                    <div className="mt-2 grid grid-cols-2 gap-2 text-sm">
                      <Cell label={cols.inputShort} value={<PriceValue value={p.input_per_million} officialValue={p.official_input_per_million} compact />} />
                      <Cell label={cols.outputShort} value={<PriceValue value={p.output_per_million} officialValue={p.official_output_per_million} compact />} />
                      <Cell label={cols.cacheReadShort} value={<PriceValue value={p.cache_read_per_million} officialValue={p.official_cache_read_per_million} compact />} />
                      <Cell label={cols.cacheWriteShort} value={<PriceValue value={p.cache_write_per_million} officialValue={p.official_cache_write_per_million} compact />} />
                    </div>
                  </div>
                ))}
              </div>
              <div className="flex flex-wrap items-center justify-between gap-3 border-t-2 border-slate-800 bg-amber-50 px-4 py-3">
                <span className="text-sm font-bold text-slate-600">
                  {c.pricingPageInfo(currentPage + 1, totalPages, providerPrices.length)}
                </span>
                <div className="flex gap-2">
                  <button
                    type="button"
                    onClick={() => setPage((value) => Math.max(0, value - 1))}
                    disabled={currentPage === 0}
                    className="rounded-full border-2 border-slate-800 bg-white px-4 py-1.5 text-sm font-bold lift disabled:opacity-40"
                  >
                    {c.pricingPrev}
                  </button>
                  <button
                    type="button"
                    onClick={() => setPage((value) => Math.min(totalPages - 1, value + 1))}
                    disabled={currentPage >= totalPages - 1}
                    className="rounded-full border-2 border-slate-800 bg-white px-4 py-1.5 text-sm font-bold lift disabled:opacity-40"
                  >
                    {c.pricingNext}
                  </button>
                </div>
              </div>
            </div>
          )}
          <div className="mt-3 text-xs text-slate-500">{c.pricingFooter}</div>
        </div>
      )}
    </section>
  );
}

function priceProviderTabs(prices: Price[] | null): string[] {
  if (!prices) return DEFAULT_PRICE_PROVIDERS;
  const dynamic = prices
    .filter((price) => price.model_pattern !== "*")
    .map((price) => price.app_type)
    .filter((provider): provider is string => Boolean(provider));
  return Array.from(new Set([...DEFAULT_PRICE_PROVIDERS, ...dynamic]));
}

function Th({ children, align }: { children: React.ReactNode; align?: "right" }) {
  return <th className={`px-4 py-3 text-xs font-bold uppercase tracking-wider text-slate-700 ${align === "right" ? "text-right" : "text-left"}`}>{children}</th>;
}

function Cell({ label, value }: { label: string; value: React.ReactNode }) {
  return (
    <div className="flex items-center justify-between rounded-xl bg-slate-50 px-2 py-1">
      <span className="text-xs text-slate-500">{label}</span>
      <span className="font-mono">{value}</span>
    </div>
  );
}

function PriceValue({
  value,
  officialValue,
  compact,
}: {
  value?: string | null;
  officialValue?: string | null;
  compact?: boolean;
}) {
  if (value === null || value === undefined || value === "") return null;
  return (
    <span className={`inline-flex ${compact ? "items-end gap-1" : "flex-col items-end"}`}>
      {officialValue ? (
        <span className="text-[0.68rem] leading-none text-slate-400 line-through">
          {formatPriceCell(officialValue)}
        </span>
      ) : null}
      <span className="font-extrabold text-slate-900">{formatPriceCell(value)}</span>
    </span>
  );
}

function formatPriceCell(value?: string | null): string {
  if (value === null || value === undefined || value === "") return "";
  return `$${trimDecimal(value)}`;
}

function comparePriceDesc(left: Price, right: Price): number {
  const outputDiff = priceNumber(right.output_per_million) - priceNumber(left.output_per_million);
  if (outputDiff !== 0) return outputDiff;
  const inputDiff = priceNumber(right.input_per_million) - priceNumber(left.input_per_million);
  if (inputDiff !== 0) return inputDiff;
  return left.model_pattern.localeCompare(right.model_pattern);
}

function priceNumber(value?: string | null): number {
  const parsed = Number(value ?? 0);
  return Number.isFinite(parsed) ? parsed : 0;
}

function formatDiscountPercent(value: string | number | null | undefined): string {
  const parsed = Number(value ?? 10);
  if (!Number.isFinite(parsed)) return "10%";
  return `${parsed.toFixed(2).replace(/0+$/, "").replace(/\.$/, "")}%`;
}

function trimDecimal(value: string): string {
  if (!value.includes(".")) return value;
  return value.replace(/(\.\d*?[1-9])0+$/, "$1").replace(/\.0+$/, "");
}

export function HowToUse() {
  const { locale } = useLocale();
  const c = copy[locale].landing;
  const apiIcons = [<KeyRound key="k" size={18} />, <TerminalSquare key="t" size={18} />, <PlugZap key="p" size={18} />];
  const apiColors = ["bg-violet-300", "bg-amber-300", "bg-emerald-300"];
  const providerIcons = [<Store key="s" size={18} />, <Sparkles key="sp" size={18} />, <Coins key="c" size={18} />, <Users key="u" size={18} />];
  const providerColors = ["bg-pink-300", "bg-violet-300", "bg-amber-300", "bg-emerald-300"];
  return (
    <section className="mx-auto max-w-6xl bg-dot-grid-soft px-6 py-12">
      <h2 className="font-display text-3xl font-extrabold md:text-4xl">{c.howToTitle}</h2>
      <p className="mt-2 text-slate-600">{c.howToSubtitle}</p>
      <div className="mt-6 grid gap-6 md:grid-cols-2">
        <div className="sticker bg-white p-6 lift">
          <h3 className="font-display text-xl font-extrabold">{c.howToApi.title}</h3>
          <ol className="mt-4 grid gap-3">
            {c.howToApi.steps.map((s, i) => (
              <li key={s.title} className="flex items-start gap-3">
                <span className={`mt-0.5 inline-flex h-7 w-7 items-center justify-center rounded-full border-2 border-slate-800 ${apiColors[i]} font-extrabold`}>{i + 1}</span>
                <div className="flex-1">
                  <div className="flex items-center gap-2">
                    <span className={`rounded-full border-2 border-slate-800 ${apiColors[i]} p-1.5`}>{apiIcons[i]}</span>
                    <span className="font-display text-lg font-extrabold">{s.title}</span>
                  </div>
                  <p className="mt-1 text-sm text-slate-600">{s.body}</p>
                </div>
              </li>
            ))}
          </ol>
        </div>
        <div className="sticker bg-white p-6 lift">
          <h3 className="font-display text-xl font-extrabold">{c.howToProvider.title}</h3>
          <ol className="mt-4 grid gap-3">
            {c.howToProvider.steps.map((s, i) => (
              <li key={s.title} className="flex items-start gap-3">
                <span className={`mt-0.5 inline-flex h-7 w-7 items-center justify-center rounded-full border-2 border-slate-800 ${providerColors[i]} font-extrabold`}>{i + 1}</span>
                <div className="flex-1">
                  <div className="flex items-center gap-2">
                    <span className={`rounded-full border-2 border-slate-800 ${providerColors[i]} p-1.5`}>{providerIcons[i]}</span>
                    <span className="font-display text-lg font-extrabold">{s.title}</span>
                  </div>
                  <p className="mt-1 text-sm text-slate-600">{s.body}</p>
                </div>
              </li>
            ))}
          </ol>
        </div>
      </div>
      <CurlExamples />
    </section>
  );
}

type CurlExampleKey = "claude" | "codex" | "gemini";

const CURL_EXAMPLE_KEYS: CurlExampleKey[] = ["claude", "codex", "gemini"];

const CURL_EXAMPLE_MODELS: Record<CurlExampleKey, string> = {
  claude: "claude-opus-4-7",
  codex: "gpt-5.5",
  gemini: "gemini-2.5-flash",
};

function buildCurlCommand(key: CurlExampleKey, baseUrl: string): string {
  const base = baseUrl.replace(/\/+$/, "");
  switch (key) {
    case "claude":
      return `curl ${base}/v1/messages \\
  -H "x-api-key: sk-cs-..." \\
  -H "anthropic-version: 2023-06-01" \\
  -H "content-type: application/json" \\
  -d '{
    "model": "claude-opus-4-7",
    "max_tokens": 1024,
    "messages": [{"role":"user","content":"hello"}],
    "stream": true
  }'`;
    case "codex":
      return `curl ${base}/v1/chat/completions \\
  -H "Authorization: Bearer sk-cs-..." \\
  -H "content-type: application/json" \\
  -d '{
    "model": "gpt-5.5",
    "messages": [{"role":"user","content":"hello"}],
    "stream": true
  }'`;
    case "gemini":
      return `curl "${base}/v1beta/models/gemini-2.5-flash:generateContent" \\
  -H "x-goog-api-key: sk-cs-..." \\
  -H "content-type: application/json" \\
  -d '{
    "contents": [{"role":"user","parts":[{"text":"hello"}]}]
  }'`;
  }
}

function useMarketBaseUrl(): string {
  const config = usePublicConfig();
  const [origin, setOrigin] = useState("");
  useEffect(() => {
    if (typeof window !== "undefined") setOrigin(window.location.origin);
  }, []);
  return config.marketPublicBaseUrl || origin || "https://your-market.example.com";
}

function CurlExamples() {
  const { locale } = useLocale();
  const c = copy[locale].landing;
  const t = c.curlTabs;
  const toast = useToast();
  const baseUrl = useMarketBaseUrl();
  const [active, setActive] = useState<CurlExampleKey>("claude");
  const [copied, setCopied] = useState(false);
  const currentCommand = buildCurlCommand(active, baseUrl);

  async function handleCopy() {
    if (typeof navigator === "undefined" || !navigator.clipboard) {
      toast.push({ variant: "error", title: t.copyFailed, description: t.copyFailedDesc });
      return;
    }
    try {
      await navigator.clipboard.writeText(currentCommand);
      setCopied(true);
      setTimeout(() => setCopied(false), 1600);
      toast.push({ variant: "success", title: t.copyToastTitle, description: t.copyToastDesc });
    } catch {
      toast.push({ variant: "error", title: t.copyFailed, description: t.copyFailedDesc });
    }
  }

  return (
    <div className="mt-6 sticker bg-white p-5 md:p-6">
      <div className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="text-xs font-extrabold uppercase tracking-wider text-slate-900">{c.howToCurlTitle}</div>
        <button
          type="button"
          onClick={handleCopy}
          aria-label={copied ? t.copied : t.copy}
          className={`inline-flex items-center gap-1.5 rounded-full border-2 border-slate-800 px-3 py-1.5 text-xs font-extrabold transition-colors ${copied ? "bg-emerald-300 text-slate-900" : "bg-white text-slate-900 hover:bg-amber-300"}`}
        >
          {copied ? <Check size={14} /> : <CopyIcon size={14} />}
          <span>{copied ? t.copied : t.copy}</span>
        </button>
      </div>
      <div role="tablist" aria-label={c.howToCurlTitle} className="mb-4 flex flex-wrap gap-2">
        {CURL_EXAMPLE_KEYS.map((key) => {
          const isActive = key === active;
          return (
            <button
              key={key}
              type="button"
              role="tab"
              aria-selected={isActive}
              onClick={() => {
                setActive(key);
                setCopied(false);
              }}
              className={`inline-flex items-center gap-2 rounded-full border-2 px-3 py-1.5 text-xs font-extrabold transition-colors ${isActive ? "border-slate-800 bg-amber-300 text-slate-900" : "border-slate-300 bg-white text-slate-700 hover:border-slate-800 hover:bg-amber-100 hover:text-slate-900"}`}
            >
              <span>{t[key]}</span>
              <span className={`rounded-full border-2 px-1.5 py-0 font-mono text-[10px] ${isActive ? "border-slate-900 bg-slate-900 text-white" : "border-slate-300 text-slate-600"}`}>{CURL_EXAMPLE_MODELS[key]}</span>
            </button>
          );
        })}
      </div>
      <pre className="overflow-auto rounded-2xl border-2 border-slate-200 bg-amber-50 p-4 text-xs leading-relaxed text-slate-900"><code>{currentCommand}</code></pre>
    </div>
  );
}

export function FinalCTA() {
  const { locale } = useLocale();
  const c = copy[locale].landing;
  return (
    <section className="mx-auto max-w-6xl px-6 pb-24 pt-12">
      <div className="relative sticker bg-white p-10 text-center text-slate-900">
        <Confetti density="medium" />
        <h2 className="relative z-10 font-display text-4xl font-extrabold text-slate-900 md:text-5xl">{c.finalCtaTitle}</h2>
        <p className="relative z-10 mx-auto mt-3 max-w-xl text-base font-medium text-slate-700 md:text-lg">{c.finalCtaSubtitle}</p>
        <div className="relative z-10 mt-6 flex flex-wrap justify-center gap-4">
          <Link href="/dashboard#keys" className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-6 py-3 font-bold text-white btn-pop">
            {c.finalCtaApi} <ArrowRight size={16} />
          </Link>
          <Link href="/claim" className="inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-amber-300 px-6 py-3 font-bold text-slate-900 btn-pop">
            {c.finalCtaProvider}
          </Link>
        </div>
      </div>
    </section>
  );
}
