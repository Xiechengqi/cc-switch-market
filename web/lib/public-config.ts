"use client";

import { useEffect, useState } from "react";
import { apiGet } from "@/lib/client-api";

export type FooterLink = {
  labelZh: string;
  labelEn: string;
  url: string;
  icon: string;
};

export type PublicConfig = {
  platformCommissionBps: number;
  platformCommissionRate: string;
  platformCommissionDecimal: string;
  marketCommissionBps: number;
  marketCommissionRate: string;
  routerCommissionBps: number;
  routerCommissionRate: string;
  totalCommissionBps: number;
  totalCommissionRate: string;
  timeZoneOffsetMinutes: number;
  adminTablePageSize: number;
  marketPublicBaseUrl: string;
  routerApiBaseUrl: string;
  cloudflareTurnstileSiteKey: string;
  footerLinks: FooterLink[];
};

const DEFAULT_CONFIG: PublicConfig = {
  platformCommissionBps: 1500,
  platformCommissionRate: "15%",
  platformCommissionDecimal: "0.1500",
  marketCommissionBps: 1000,
  marketCommissionRate: "10%",
  routerCommissionBps: 500,
  routerCommissionRate: "5%",
  totalCommissionBps: 1500,
  totalCommissionRate: "15%",
  timeZoneOffsetMinutes: 480,
  adminTablePageSize: 20,
  marketPublicBaseUrl: "",
  routerApiBaseUrl: "",
  cloudflareTurnstileSiteKey: "",
  footerLinks: [],
};

let cachedConfig: PublicConfig | null = null;
const configListeners = new Set<(config: PublicConfig) => void>();

export function usePublicConfig(): PublicConfig {
  const [config, setConfig] = useState<PublicConfig>(cachedConfig ?? DEFAULT_CONFIG);

  useEffect(() => {
    let cancelled = false;
    if (cachedConfig) return;
    apiGet<PublicConfig>("/v1/public/config")
      .then((value) => {
        cachedConfig = normalizePublicConfig(value);
        if (!cancelled) setConfig(cachedConfig);
        notifyConfigListeners(cachedConfig);
      })
      .catch(() => {
        cachedConfig = DEFAULT_CONFIG;
        if (!cancelled) setConfig(DEFAULT_CONFIG);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return config;
}

export function updateCachedPublicConfig(value: Partial<PublicConfig>) {
  cachedConfig = normalizePublicConfig({ ...(cachedConfig ?? DEFAULT_CONFIG), ...value });
  notifyConfigListeners(cachedConfig);
}

export function usePublicConfigSubscription(onChange: (config: PublicConfig) => void) {
  useEffect(() => {
    configListeners.add(onChange);
    return () => {
      configListeners.delete(onChange);
    };
  }, [onChange]);
}

export function formatCommissionRate(bps: unknown): string {
  const value = typeof bps === "number" ? bps : Number(bps ?? DEFAULT_CONFIG.platformCommissionBps);
  if (!Number.isFinite(value)) return DEFAULT_CONFIG.platformCommissionRate;
  const percent = value / 100;
  if (value % 100 === 0) return `${percent.toFixed(0)}%`;
  if (value % 10 === 0) return `${percent.toFixed(1)}%`;
  return `${percent.toFixed(2)}%`;
}

export function commissionText(locale: "zh" | "en", bps: number) {
  const rate = formatCommissionRate(bps);
  if (locale === "zh") {
    return {
      rate,
      short: `${rate} 平台抽成`,
      heroBadge: `官方参考价 1/10 · ${rate} 总抽成 · 30 秒上手`,
      featureTitle: `${rate} 平台抽成`,
      featureBody: `用户消费按公开价格结算，${rate} 进入平台收入，其余净额归 Provider。`,
      pricingBadge: `官方价 1/10 · ${rate} 总抽成`,
      pricingSubtitle: `Market 调用价按官方参考价 1/10 展示；Provider 收入 = 用户消费金额 - ${rate} 总抽成。`,
      claimNote: `当前总抽成为 ${rate}，收益按 gross - Market fee - Router fee 入账。`,
      adminLabel: "平台抽成",
    };
  }
  return {
    rate,
    short: `${rate} platform take`,
    heroBadge: `1/10 official reference price · ${rate} total take · live in 30s`,
    featureTitle: `${rate} platform take`,
    featureBody: `User spend is billed at public prices; ${rate} goes to platform revenue and the rest is provider net income.`,
    pricingBadge: `1/10 official price · ${rate} total take`,
    pricingSubtitle: `Market calls are shown at 1/10 of the official reference price; provider income = user spend - ${rate} total take.`,
    claimNote: `Current total take is ${rate}. Earnings are posted as gross - Market fee - Router fee.`,
    adminLabel: "Platform take",
  };
}

function normalizePublicConfig(value: PublicConfig): PublicConfig {
  const marketBps = Number(value.marketCommissionBps ?? DEFAULT_CONFIG.marketCommissionBps);
  const routerBps = Number(value.routerCommissionBps ?? DEFAULT_CONFIG.routerCommissionBps);
  const totalBps = Number(value.totalCommissionBps ?? value.platformCommissionBps ?? marketBps + routerBps);
  if (!Number.isFinite(totalBps)) return DEFAULT_CONFIG;
  const timeZoneOffsetMinutes = Number(value.timeZoneOffsetMinutes);
  return {
    platformCommissionBps: totalBps,
    platformCommissionRate: value.platformCommissionRate || formatCommissionRate(totalBps),
    platformCommissionDecimal: value.platformCommissionDecimal || (totalBps / 10_000).toFixed(4),
    marketCommissionBps: Number.isFinite(marketBps) ? marketBps : DEFAULT_CONFIG.marketCommissionBps,
    marketCommissionRate: value.marketCommissionRate || formatCommissionRate(marketBps),
    routerCommissionBps: Number.isFinite(routerBps) ? routerBps : DEFAULT_CONFIG.routerCommissionBps,
    routerCommissionRate: value.routerCommissionRate || formatCommissionRate(routerBps),
    totalCommissionBps: totalBps,
    totalCommissionRate: value.totalCommissionRate || formatCommissionRate(totalBps),
    timeZoneOffsetMinutes: Number.isFinite(timeZoneOffsetMinutes) ? timeZoneOffsetMinutes : DEFAULT_CONFIG.timeZoneOffsetMinutes,
    adminTablePageSize: normalizeAdminTablePageSize(value.adminTablePageSize),
    marketPublicBaseUrl: typeof value.marketPublicBaseUrl === "string" ? value.marketPublicBaseUrl : DEFAULT_CONFIG.marketPublicBaseUrl,
    routerApiBaseUrl: typeof value.routerApiBaseUrl === "string" ? value.routerApiBaseUrl : DEFAULT_CONFIG.routerApiBaseUrl,
    cloudflareTurnstileSiteKey: typeof value.cloudflareTurnstileSiteKey === "string" ? value.cloudflareTurnstileSiteKey : "",
    footerLinks: Array.isArray(value.footerLinks)
      ? value.footerLinks
          .filter((item): item is FooterLink => !!item && typeof item === "object")
          .map((item) => ({
            labelZh: typeof item.labelZh === "string" ? item.labelZh : "",
            labelEn: typeof item.labelEn === "string" ? item.labelEn : "",
            url: typeof item.url === "string" ? item.url : "",
            icon: typeof item.icon === "string" ? item.icon : "link",
          }))
      : [],
  };
}

function normalizeAdminTablePageSize(value: number | undefined): number {
  const pageSize = Number(value ?? DEFAULT_CONFIG.adminTablePageSize);
  if (!Number.isFinite(pageSize)) return DEFAULT_CONFIG.adminTablePageSize;
  return Math.min(500, Math.max(1, Math.floor(pageSize)));
}

function notifyConfigListeners(config: PublicConfig) {
  for (const listener of configListeners) listener(config);
}
