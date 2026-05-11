"use client";

import { useEffect, useState, type ReactNode } from "react";
import { ShieldAlert, KeyRound } from "lucide-react";
import { useMarketAuth } from "@/components/auth";

type AdminGuardProps = {
  children: ReactNode;
};

export function AdminGuard({ children }: AdminGuardProps) {
  const { user, loading, showLogin } = useMarketAuth();
  const [redirected, setRedirected] = useState(false);

  // 非 admin 已登录用户：跳走
  useEffect(() => {
    if (loading || redirected || typeof window === "undefined") return;
    if (user && !user.isAdmin) {
      setRedirected(true);
      window.location.replace(`/dashboard?denied=admin`);
    }
  }, [user, loading, redirected]);

  // 未登录：自动弹出登录窗
  useEffect(() => {
    if (loading) return;
    if (!user) showLogin();
  }, [loading, user, showLogin]);

  if (loading) {
    return (
      <div className="mx-auto max-w-6xl px-6 py-24">
        <div className="sticker animate-pulse p-8 text-center">
          <div className="font-display text-2xl font-extrabold">正在校验管理员权限…</div>
        </div>
      </div>
    );
  }

  if (!user) {
    return (
      <div className="mx-auto max-w-md px-6 py-16">
        <div className="sticker bg-amber-50 p-8 text-center">
          <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full border-2 border-slate-800 bg-amber-300">
            <KeyRound size={24} />
          </div>
          <div className="font-display text-2xl font-extrabold">需要先登录</div>
          <div className="mt-2 text-sm text-slate-600">请使用管理员邮箱登录后再访问此页面</div>
          <button
            onClick={showLogin}
            className="mt-4 inline-flex items-center gap-2 rounded-full border-2 border-slate-800 bg-violet-500 px-5 py-2 font-bold text-white btn-pop"
          >
            打开登录窗口
          </button>
        </div>
      </div>
    );
  }

  if (!user.isAdmin) {
    return (
      <div className="mx-auto max-w-md px-6 py-16">
        <div className="sticker bg-pink-50 p-8 text-center">
          <div className="mx-auto mb-4 flex h-14 w-14 items-center justify-center rounded-full border-2 border-slate-800 bg-pink-300">
            <ShieldAlert size={24} />
          </div>
          <div className="font-display text-2xl font-extrabold">权限不足</div>
          <div className="mt-2 text-sm text-slate-600">此页面仅管理员可访问</div>
        </div>
      </div>
    );
  }

  return <>{children}</>;
}
