import type { ReactNode } from "react";

type Color = "violet" | "pink" | "amber" | "emerald";

const COLOR_MAP: Record<Color, { icon: string; shadow: string; ring: string }> = {
  violet: { icon: "bg-violet-500 text-white", shadow: "shadow-violet", ring: "bg-violet-100" },
  pink: { icon: "bg-pink-400 text-white", shadow: "shadow-pink", ring: "bg-pink-100" },
  amber: { icon: "bg-amber-300 text-slate-800", shadow: "shadow-amber", ring: "bg-amber-100" },
  emerald: { icon: "bg-emerald-400 text-white", shadow: "shadow-emerald", ring: "bg-emerald-100" }
};

type StatCardProps = {
  label: string;
  value: ReactNode;
  sublabel?: ReactNode;
  icon?: ReactNode;
  color?: Color;
  loading?: boolean;
};

export function StatCard({ label, value, sublabel, icon, color = "violet", loading }: StatCardProps) {
  const c = COLOR_MAP[color];
  return (
    <div className={`relative rounded-3xl border-2 border-slate-800 bg-white p-5 ${c.shadow} lift`}>
      {icon && (
        <div className={`absolute -top-4 -right-3 rounded-full border-2 border-slate-800 ${c.icon} p-2`}>
          {icon}
        </div>
      )}
      <div className="text-xs font-bold uppercase tracking-wider text-slate-500">{label}</div>
      <div className="mt-2 font-display text-3xl font-extrabold md:text-4xl">
        {loading ? <span className={`inline-block h-9 w-28 rounded-lg ${c.ring} animate-pulse`} /> : value}
      </div>
      {sublabel && <div className="mt-1 text-sm text-slate-500">{sublabel}</div>}
    </div>
  );
}
