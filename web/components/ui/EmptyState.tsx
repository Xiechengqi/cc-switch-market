import type { ReactNode } from "react";

type Shape = "circle" | "triangle" | "square" | "blob";

type EmptyStateProps = {
  shape?: Shape;
  title: string;
  hint?: string;
  action?: ReactNode;
};

function ShapeArt({ shape }: { shape: Shape }) {
  const base = "h-20 w-20 border-2 border-slate-800";
  if (shape === "triangle") {
    return (
      <svg viewBox="0 0 80 80" className="h-20 w-20">
        <polygon points="40,6 76,72 4,72" fill="#fbbf24" stroke="#1e293b" strokeWidth={3} strokeLinejoin="round" />
      </svg>
    );
  }
  if (shape === "square") {
    return <div className={`${base} bg-pink-300 rotate-6 rounded-2xl`} />;
  }
  if (shape === "blob") {
    return (
      <div
        className={`${base} bg-emerald-300`}
        style={{ borderRadius: "60% 40% 50% 60% / 50% 60% 40% 50%" }}
      />
    );
  }
  return <div className={`${base} bg-violet-400 rounded-full`} />;
}

export function EmptyState({ shape = "circle", title, hint, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center gap-4 rounded-3xl border-2 border-dashed border-slate-300 bg-white/60 p-10 text-center">
      <div className="animate-float"><ShapeArt shape={shape} /></div>
      <div>
        <div className="font-display text-xl font-extrabold">{title}</div>
        {hint && <div className="mt-1 text-sm text-slate-500">{hint}</div>}
      </div>
      {action}
    </div>
  );
}
