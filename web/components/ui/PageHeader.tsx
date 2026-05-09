import type { ReactNode } from "react";

type PageHeaderProps = {
  title: string;
  subtitle?: ReactNode;
  actions?: ReactNode;
  badge?: ReactNode;
};

export function PageHeader({ title, subtitle, actions, badge }: PageHeaderProps) {
  return (
    <div className="flex flex-col gap-4 md:flex-row md:items-end md:justify-between">
      <div className="flex flex-col gap-2">
        <div className="flex items-center gap-3">
          <span className="inline-block h-3 w-3 rounded-full bg-violet-500 border-2 border-slate-800" aria-hidden />
          <h1 className="font-display text-4xl font-extrabold md:text-5xl">{title}</h1>
          {badge}
        </div>
        {subtitle && <div className="text-slate-600">{subtitle}</div>}
      </div>
      {actions && <div className="flex flex-wrap items-center gap-3">{actions}</div>}
    </div>
  );
}
