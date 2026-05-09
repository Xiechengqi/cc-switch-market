"use client";

import { CheckCircle2, AlertCircle, Info, AlertTriangle } from "lucide-react";
import { createContext, useCallback, useContext, useState, type ReactNode } from "react";

type Variant = "success" | "error" | "info" | "warning";

type ToastItem = {
  id: number;
  variant: Variant;
  title: string;
  description?: string;
};

type ToastContextValue = {
  push: (toast: { variant?: Variant; title: string; description?: string }) => void;
};

const ToastContext = createContext<ToastContextValue>({ push: () => {} });

const STYLE: Record<Variant, { bg: string; icon: ReactNode }> = {
  success: { bg: "bg-emerald-100 border-slate-800", icon: <CheckCircle2 size={20} /> },
  error: { bg: "bg-pink-200 border-slate-800", icon: <AlertCircle size={20} /> },
  info: { bg: "bg-violet-100 border-slate-800", icon: <Info size={20} /> },
  warning: { bg: "bg-amber-200 border-slate-800", icon: <AlertTriangle size={20} /> }
};

export function ToastProvider({ children }: { children: ReactNode }) {
  const [items, setItems] = useState<ToastItem[]>([]);

  const push = useCallback((toast: { variant?: Variant; title: string; description?: string }) => {
    const id = Date.now() + Math.random();
    const variant = toast.variant ?? "info";
    setItems((prev) => [...prev, { id, variant, title: toast.title, description: toast.description }]);
    setTimeout(() => {
      setItems((prev) => prev.filter((item) => item.id !== id));
    }, 4200);
  }, []);

  return (
    <ToastContext.Provider value={{ push }}>
      {children}
      <div className="pointer-events-none fixed bottom-4 right-4 z-[60] flex flex-col gap-3">
        {items.map((item) => {
          const s = STYLE[item.variant];
          return (
            <div
              key={item.id}
              className={`pointer-events-auto flex w-80 max-w-[92vw] items-start gap-3 rounded-2xl border-2 ${s.bg} p-4 animate-pop-in`}
            >
              <div className="mt-0.5 shrink-0">{s.icon}</div>
              <div className="min-w-0 flex-1">
                <div className="font-bold">{item.title}</div>
                {item.description && <div className="mt-1 break-words text-sm text-slate-700">{item.description}</div>}
              </div>
            </div>
          );
        })}
      </div>
    </ToastContext.Provider>
  );
}

export function useToast() {
  return useContext(ToastContext);
}
