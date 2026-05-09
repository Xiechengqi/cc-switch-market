"use client";

import { useEffect, useState, type ReactNode } from "react";
import { Tabs as ShadTabs, TabsContent, TabsList, TabsTrigger } from "@/components/shadcn/tabs";
import { cn } from "@/lib/cn";

export type TabItem = {
  key: string;
  label: string;
  icon?: ReactNode;
  badge?: ReactNode;
};

type TabsProps = {
  items: TabItem[];
  defaultKey?: string;
  storageKey?: string;
  children: (active: string) => ReactNode;
};

const ACTIVE_COLORS = [
  "data-[state=active]:bg-violet-500 data-[state=active]:text-white",
  "data-[state=active]:bg-pink-400 data-[state=active]:text-white",
  "data-[state=active]:bg-amber-300 data-[state=active]:text-slate-900",
  "data-[state=active]:bg-emerald-400 data-[state=active]:text-white",
];

export function Tabs({ items, defaultKey, storageKey, children }: TabsProps) {
  const [active, setActive] = useState<string>(() => defaultKey ?? items[0]?.key ?? "");

  useEffect(() => {
    function isValidKey(key: string) {
      return items.some((item) => item.key === key);
    }

    function activate(key: string) {
      setActive(key);
      if (storageKey && typeof window !== "undefined") {
        window.localStorage.setItem(storageKey, key);
      }
    }

    function readHash() {
      const hash = typeof window !== "undefined" ? window.location.hash.replace(/^#/, "") : "";
      const stored = storageKey && typeof window !== "undefined" ? window.localStorage.getItem(storageKey) ?? "" : "";
      if (hash && isValidKey(hash)) {
        activate(hash);
      } else if (stored && isValidKey(stored)) {
        setActive(stored);
      } else if (defaultKey && isValidKey(defaultKey)) {
        setActive(defaultKey);
      } else if (items[0]) {
        setActive(items[0].key);
      }
    }
    readHash();
    window.addEventListener("hashchange", readHash);
    return () => window.removeEventListener("hashchange", readHash);
  }, [items, defaultKey, storageKey]);

  function go(key: string) {
    if (typeof window !== "undefined") {
      const url = `${window.location.pathname}#${key}`;
      window.history.replaceState(null, "", url);
      if (storageKey) {
        window.localStorage.setItem(storageKey, key);
      }
    }
    setActive(key);
  }

  return (
    <ShadTabs value={active} onValueChange={go} className="grid gap-6">
      <TabsList className="rounded-3xl p-2">
        {items.map((item, index) => (
          <TabsTrigger
            key={item.key}
            value={item.key}
            className={cn(
              "border-2 border-slate-800 text-sm font-bold lift data-[state=active]:border-slate-800",
              ACTIVE_COLORS[index % ACTIVE_COLORS.length]
            )}
          >
            {item.icon}
            <span>{item.label}</span>
            {item.badge}
          </TabsTrigger>
        ))}
      </TabsList>
      {items.map((item) => (
        <TabsContent key={item.key} value={item.key} className="animate-slide-up">
          {children(item.key)}
        </TabsContent>
      ))}
    </ShadTabs>
  );
}
