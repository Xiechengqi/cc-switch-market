"use client";

import { useLocale } from "@/components/language-provider";

export function LanguageSwitch() {
  const { locale, setLocale } = useLocale();
  return (
    <div className="inline-flex items-center rounded-full border-2 border-[var(--border)] bg-[var(--card)] p-1">
      <button
        type="button"
        onClick={() => setLocale("zh")}
        className={`rounded-full px-3 py-1 text-xs font-bold ${locale === "zh" ? "bg-violet-500 text-white" : "text-[var(--foreground)]"}`}
      >
        ZH
      </button>
      <button
        type="button"
        onClick={() => setLocale("en")}
        className={`rounded-full px-3 py-1 text-xs font-bold ${locale === "en" ? "bg-amber-300 text-[var(--foreground)]" : "text-[var(--foreground)]"}`}
      >
        EN
      </button>
    </div>
  );
}
