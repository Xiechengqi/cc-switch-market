import type { ReactNode } from "react";

type Variant =
  | "pending"
  | "processing"
  | "paid"
  | "failed"
  | "review"
  | "info"
  | "neutral"
  | "success"
  | "warning";

const STYLE: Record<Variant, string> = {
  pending: "bg-amber-200 border-slate-800",
  processing: "bg-violet-200 border-slate-800",
  paid: "bg-emerald-300 border-slate-800",
  success: "bg-emerald-300 border-slate-800",
  failed: "bg-pink-300 border-slate-800",
  review: "bg-pink-200 border-slate-800",
  info: "bg-sky-200 border-slate-800",
  warning: "bg-amber-300 border-slate-800",
  neutral: "bg-slate-100 border-slate-800"
};

const STATUS_TEXT: Record<string, { variant: Variant; label: string }> = {
  pending: { variant: "pending", label: "待处理" },
  processing: { variant: "processing", label: "处理中" },
  paid: { variant: "paid", label: "已完成" },
  failed: { variant: "failed", label: "失败" },
  cancelled: { variant: "neutral", label: "已取消" },
  needs_review: { variant: "review", label: "待复核" },
  active: { variant: "success", label: "活跃" },
  inactive: { variant: "neutral", label: "已停用" },
  open: { variant: "info", label: "已开启" },
  resolved: { variant: "success", label: "已解决" },
  closed: { variant: "neutral", label: "已关闭" },
  waiting_user: { variant: "pending", label: "等待用户" },
  waiting_admin: { variant: "info", label: "等待管理员" },
  reserved: { variant: "processing", label: "已锁定" },
  streaming: { variant: "processing", label: "流式中" },
  settled: { variant: "paid", label: "已结算" },
  failed_released: { variant: "neutral", label: "已释放" },
  failed_charged: { variant: "failed", label: "失败已扣" },
  refunded: { variant: "neutral", label: "已退款" },
  expired: { variant: "neutral", label: "已过期" },
  chargeback: { variant: "failed", label: "拒付" }
};

type PillProps = {
  variant?: Variant;
  status?: string;
  children?: ReactNode;
  size?: "sm" | "md";
};

export function Pill({ variant, status, children, size = "md" }: PillProps) {
  let v: Variant = variant ?? "neutral";
  let label: ReactNode = children;
  if (status && STATUS_TEXT[status]) {
    v = variant ?? STATUS_TEXT[status].variant;
    label = label ?? STATUS_TEXT[status].label;
  } else if (status && !label) {
    label = status;
  }
  const sizing = size === "sm" ? "px-2 py-0.5 text-xs" : "px-3 py-1 text-sm";
  return (
    <span className={`inline-flex items-center gap-1 rounded-full border-2 font-bold ${STYLE[v]} ${sizing}`}>
      {label}
    </span>
  );
}
