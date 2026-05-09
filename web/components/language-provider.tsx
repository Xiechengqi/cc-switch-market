"use client";

import { createContext, useContext, useEffect, useMemo, useState, type ReactNode } from "react";
import { messages, type Locale, type MessageTree } from "@/lib/i18n";

type LanguageContextValue = {
  locale: Locale;
  setLocale: (locale: Locale) => void;
  t: MessageTree;
};

const STORAGE_KEY = "cc_switch_market_locale";

const LanguageContext = createContext<LanguageContextValue>({
  locale: "zh",
  setLocale: () => {},
  t: messages.zh
});

export function LanguageProvider({ children }: { children: ReactNode }) {
  const [locale, setLocaleState] = useState<Locale>("zh");

  useEffect(() => {
    if (typeof window === "undefined") return;
    const saved = window.localStorage.getItem(STORAGE_KEY);
    if (saved === "zh" || saved === "en") {
      setLocaleState(saved);
    }
  }, []);

  useEffect(() => {
    if (typeof document === "undefined") return;
    document.documentElement.lang = locale === "zh" ? "zh-CN" : "en";
    window.localStorage.setItem(STORAGE_KEY, locale);
  }, [locale]);

  const value = useMemo<LanguageContextValue>(() => ({
    locale,
    setLocale: setLocaleState,
    t: messages[locale] as MessageTree
  }), [locale]);

  return <LanguageContext.Provider value={value}>{children}</LanguageContext.Provider>;
}

export function useLocale() {
  return useContext(LanguageContext);
}
