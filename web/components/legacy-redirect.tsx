"use client";

import { useEffect } from "react";

const REDIRECTS: Record<string, string> = {
  "/login": "/?login=1",
  "/pricing": "/#pricing",
  "/usage": "/dashboard#usage",
  "/admin/users": "/admin#users",
  "/admin/topups": "/admin#money",
  "/admin/prices": "/admin#models",
  "/admin/shares": "/admin#shares",
  "/admin/charges": "/admin#money",
  "/admin/earnings": "/admin#money",
  "/admin/payout-requests": "/admin#money",
  "/admin/settlements": "/admin#money",
  "/admin/tickets": "/admin#tickets",
  "/admin/ledger": "/admin#money",
  "/admin/money-events": "/admin#money",
  "/admin/audit": "/admin#audit"
};

export function LegacyRedirect() {
  useEffect(() => {
    if (typeof window === "undefined") return;
    const path = window.location.pathname.replace(/\/$/, "") || "/";
    const target = REDIRECTS[path];
    if (target) {
      window.location.replace(target);
    }
  }, []);
  return null;
}
