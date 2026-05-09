"use client";

import { createContext, useCallback, useContext, useEffect, useState } from "react";
import { useForm } from "react-hook-form";
import { ArrowLeft, Send, ShieldCheck } from "lucide-react";
import { Modal } from "@/components/ui/Modal";
import { useToast } from "@/components/ui/Toast";
import { useLocale } from "@/components/language-provider";
import { usePublicConfig } from "@/lib/public-config";
import { Form, FormControl, FormField, FormItem, FormLabel, FormMessage, FormDescription } from "@/components/shadcn/form";

export type MarketUser = {
  id: string;
  email: string;
  isAdmin: boolean;
};

type SessionStatus = {
  authenticated: boolean;
  user?: MarketUser | null;
};

type AuthContextValue = {
  user: MarketUser | null;
  loading: boolean;
  refresh: (preserveCurrentUser?: boolean) => Promise<void>;
  logout: () => Promise<void>;
  showLogin: () => void;
  hideLogin: () => void;
};

const AuthContext = createContext<AuthContextValue>({
  user: null,
  loading: true,
  refresh: async () => {},
  logout: async () => {},
  showLogin: () => {},
  hideLogin: () => {}
});

export function MarketAuthProvider({ children }: { children: React.ReactNode }) {
  const [user, setUser] = useState<MarketUser | null>(null);
  const [loading, setLoading] = useState(true);
  const [loginOpen, setLoginOpen] = useState(false);

  const refresh = useCallback(async (preserveCurrentUser = false) => {
    setLoading(true);
    try {
      const res = await fetch("/market-api/session/status", {
        credentials: "include",
        cache: "no-store"
      });
      if (!res.ok) {
        if (!preserveCurrentUser) setUser(null);
        return;
      }
      const status = (await res.json()) as SessionStatus;
      if (status.authenticated) {
        setUser(status.user ?? null);
      } else if (!preserveCurrentUser) {
        setUser(null);
      }
    } catch {
      if (!preserveCurrentUser) setUser(null);
    } finally {
      setLoading(false);
    }
  }, []);

  const logout = useCallback(async () => {
    await fetch("/market-api/auth/logout", {
      method: "POST",
      credentials: "include",
      headers: csrfHeader()
    }).catch(() => {});
    setUser(null);
  }, []);

  const showLogin = useCallback(() => setLoginOpen(true), []);
  const hideLogin = useCallback(() => setLoginOpen(false), []);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  useEffect(() => {
    if (typeof window === "undefined") return;
    const params = new URLSearchParams(window.location.search);
    if (params.get("login") === "1") {
      setLoginOpen(true);
      params.delete("login");
      const search = params.toString();
      const url = `${window.location.pathname}${search ? `?${search}` : ""}${window.location.hash}`;
      window.history.replaceState(null, "", url);
    }
  }, []);

  async function handleLoginSuccess(nextUser: MarketUser) {
    setUser(nextUser);
    setLoading(false);
    hideLogin();
    window.setTimeout(() => {
      void refresh(true);
    }, 750);
  }

  return (
    <AuthContext.Provider value={{ user, loading, refresh, logout, showLogin, hideLogin }}>
      {children}
      <LoginModal open={loginOpen} onClose={hideLogin} onSuccess={handleLoginSuccess} />
    </AuthContext.Provider>
  );
}

function csrfHeader(): Record<string, string> {
  if (typeof document === "undefined") return {};
  const token = document.cookie
    .split(";")
    .map((part) => part.trim())
    .find((part) => part.startsWith("cc_switch_market_csrf="))
    ?.split("=")
    .slice(1)
    .join("=");
  return token ? { "x-csrf-token": decodeURIComponent(token) } : {};
}

export function useMarketAuth() {
  return useContext(AuthContext);
}

export function AuthSlot() {
  const { user, loading, logout, showLogin } = useMarketAuth();
  const { t } = useLocale();

  if (loading) {
    return (
      <button className="rounded-full border-2 border-[var(--border)] bg-[var(--card)] px-4 py-2 text-sm font-bold text-fg" disabled>
        {t.auth.checking}
      </button>
    );
  }

  if (!user) {
    return (
      <button
        onClick={showLogin}
        className="rounded-full border-2 border-[var(--border)] bg-violet-500 px-5 py-2 text-sm font-bold text-white btn-pop"
      >
        {t.auth.login}
      </button>
    );
  }

  return (
    <div className="flex items-center gap-2">
      <span className="hidden max-w-[180px] truncate text-sm font-bold text-fg lg:inline">{user.email}</span>
      {user.isAdmin && (
        <span className="hidden rounded-full border-2 border-[var(--border)] bg-amber-300 px-2 py-0.5 text-xs font-bold text-slate-900 lg:inline">
          {t.auth.admin}
        </span>
      )}
      <button
        className="rounded-full border-2 border-[var(--border)] bg-[var(--card)] px-3 py-1.5 text-sm font-bold lift text-fg"
        onClick={logout}
      >
        {t.auth.logout}
      </button>
    </div>
  );
}

type RequestCodeResponse = { maskedDestination: string; cooldownSecs: number };
type VerifyResponse = { user: MarketUser };

type LoginFormValues = { email: string; code: string };

function LoginModal({ open, onClose, onSuccess }: { open: boolean; onClose: () => void; onSuccess: (user: MarketUser) => Promise<void> | void }) {
  const toast = useToast();
  const { t, locale } = useLocale();
  const publicConfig = usePublicConfig();
  const form = useForm<LoginFormValues>({
    defaultValues: { email: "", code: "" },
    mode: "onSubmit",
  });
  const [step, setStep] = useState<"email" | "code">("email");
  const [hint, setHint] = useState<string | null>(null);
  const [loading, setLoading] = useState(false);
  const [turnstileToken, setTurnstileToken] = useState("");
  const turnstileSiteKey = publicConfig.cloudflareTurnstileSiteKey.trim();

  useEffect(() => {
    if (!open) {
      setStep("email");
      form.reset({ email: "", code: "" });
      setHint(null);
      setLoading(false);
      setTurnstileToken("");
    }
  }, [open, form]);

  function setRootError(message: string) {
    form.setError("root.serverError", { type: "server", message });
  }

  async function requestCode(values: LoginFormValues) {
    if (turnstileSiteKey && !turnstileToken) {
      setRootError(t.auth.turnstileRequired);
      return;
    }
    setLoading(true);
    form.clearErrors("root.serverError");
    setHint(null);
    try {
      const res = await fetch("/market-api/auth/email/request-code", {
        method: "POST",
        headers: { "content-type": "application/json" },
        credentials: "include",
        cache: "no-store",
        body: JSON.stringify({ email: values.email, turnstileToken })
      });
      if (!res.ok) throw new Error(await errorMessage(res));
      const body = (await res.json()) as RequestCodeResponse;
      setStep("code");
      form.setValue("code", "");
      setHint(locale === "zh"
        ? `验证码已发送至 ${body.maskedDestination}（${body.cooldownSecs}s 内不可重发）`
        : `Code sent to ${body.maskedDestination} (${body.cooldownSecs}s cooldown)`);
    } catch (err) {
      setRootError(locale === "zh"
        ? `发送失败：${String(err).replace(/^Error:\s*/, "")}`
        : `Send failed: ${String(err).replace(/^Error:\s*/, "")}`);
    } finally {
      setLoading(false);
    }
  }

  async function verifyCode(values: LoginFormValues) {
    setLoading(true);
    form.clearErrors("root.serverError");
    try {
      const res = await fetch("/market-api/auth/email/verify-code", {
        method: "POST",
        headers: { "content-type": "application/json" },
        credentials: "include",
        cache: "no-store",
        body: JSON.stringify({ email: values.email, code: values.code })
      });
      if (!res.ok) throw new Error(await errorMessage(res));
      const body = (await res.json()) as VerifyResponse;
      toast.push({ variant: "success", title: t.auth.loginSuccess, description: body.user.email });
      await onSuccess(body.user);
    } catch (err) {
      setRootError(locale === "zh"
        ? `登录失败：${String(err).replace(/^Error:\s*/, "")}`
        : `Login failed: ${String(err).replace(/^Error:\s*/, "")}`);
    } finally {
      setLoading(false);
    }
  }

  const onSubmit = step === "email" ? requestCode : verifyCode;
  const emailValue = form.watch("email");
  const codeValue = form.watch("code");
  const rootError = form.formState.errors.root?.serverError?.message;

  return (
    <Modal
      open={open}
      onClose={onClose}
      title={t.auth.loginTitle}
      description={step === "email" ? t.auth.loginDescEmail : t.auth.loginDescCode}
      width="md"
    >
      <Form {...form}>
        <form onSubmit={form.handleSubmit(onSubmit)} className="grid gap-4 text-fg">
          <FormField
            control={form.control}
            name="email"
            rules={{
              required: true,
              pattern: { value: /.+@.+/, message: "" },
            }}
            render={({ field }) => (
              <FormItem>
                <FormLabel className="text-xs font-bold uppercase tracking-wider text-muted">{t.auth.email}</FormLabel>
                <FormControl>
                  <input
                    {...field}
                    type="email"
                    inputMode="email"
                    autoComplete="email"
                    placeholder="you@example.com"
                    disabled={loading || step === "code"}
                    autoFocus={step === "email"}
                    className="block w-full min-w-0 rounded-2xl border-2 border-[var(--border)] bg-amber-50 px-4 py-3 outline-none focus:bg-white text-fg disabled:opacity-70"
                  />
                </FormControl>
                <FormMessage className="text-xs font-bold text-pink-600" />
              </FormItem>
            )}
          />
          {step === "email" && turnstileSiteKey && (
            <TurnstileBox
              siteKey={turnstileSiteKey}
              locale={locale}
              onToken={setTurnstileToken}
            />
          )}
          {step === "code" && (
            <FormField
              control={form.control}
              name="code"
              rules={{ required: true, minLength: 6, maxLength: 6 }}
              render={({ field }) => (
                <FormItem>
                  <FormLabel className="text-xs font-bold uppercase tracking-wider text-muted">{t.auth.code}</FormLabel>
                  <FormControl>
                    <input
                      {...field}
                      onChange={(e) => field.onChange(e.target.value.replace(/\D/g, "").slice(0, 6))}
                      placeholder="000000"
                      inputMode="numeric"
                      autoComplete="one-time-code"
                      autoFocus
                      className="block w-full min-w-0 rounded-2xl border-2 border-[var(--border)] bg-amber-50 px-4 py-3 text-center text-2xl tracking-[0.5em] outline-none focus:bg-white text-fg"
                    />
                  </FormControl>
                  <FormDescription className="sr-only">6 digits</FormDescription>
                  <FormMessage className="text-xs font-bold text-pink-600" />
                </FormItem>
              )}
            />
          )}
          {hint && (
            <div className="flex items-start gap-3 rounded-2xl border-2 border-[var(--border)] bg-emerald-100 p-3 text-sm font-bold text-emerald-900">
              <ShieldCheck size={18} className="mt-0.5 shrink-0" /> <span>{hint}</span>
            </div>
          )}
          {rootError && (
            <div role="alert" className="rounded-2xl border-2 border-[var(--border)] bg-pink-200 p-3 text-sm font-bold text-pink-950">
              {rootError}
            </div>
          )}
          <div className="flex flex-wrap justify-end gap-3">
            {step === "email" ? (
              <button
                type="submit"
                className="inline-flex items-center gap-2 rounded-full border-2 border-[var(--border)] bg-violet-500 px-6 py-3 font-bold text-white btn-pop disabled:opacity-50"
                disabled={loading || !emailValue.includes("@")}
              >
                <Send size={16} /> {t.auth.sendCode}
              </button>
            ) : (
              <>
                <button
                  type="button"
                  className="inline-flex items-center gap-2 rounded-full border-2 border-[var(--border)] bg-[var(--card)] px-6 py-3 font-bold lift text-fg"
                  onClick={() => {
                    setStep("email");
                    form.setValue("code", "");
                    form.clearErrors("root.serverError");
                    setHint(null);
                  }}
                  disabled={loading}
                >
                  <ArrowLeft size={16} /> {t.auth.changeEmail}
                </button>
                <button
                  type="submit"
                  className="inline-flex items-center gap-2 rounded-full border-2 border-[var(--border)] bg-violet-500 px-6 py-3 font-bold text-white btn-pop disabled:opacity-50"
                  disabled={loading || codeValue.length !== 6}
                >
                  <ShieldCheck size={16} /> {t.auth.verifyLogin}
                </button>
              </>
            )}
          </div>
        </form>
      </Form>
    </Modal>
  );
}

type TurnstileWindow = Window & {
  turnstile?: {
    render: (element: HTMLElement, options: Record<string, unknown>) => string;
    reset: (widgetId?: string) => void;
    remove: (widgetId: string) => void;
  };
};

let turnstileScriptPromise: Promise<void> | null = null;

function loadTurnstileScript(): Promise<void> {
  if (typeof window === "undefined") return Promise.resolve();
  if ((window as TurnstileWindow).turnstile) return Promise.resolve();
  if (turnstileScriptPromise) return turnstileScriptPromise;
  turnstileScriptPromise = new Promise((resolve, reject) => {
    const existing = document.querySelector<HTMLScriptElement>("script[data-turnstile-script='1']");
    if (existing) {
      existing.addEventListener("load", () => resolve(), { once: true });
      existing.addEventListener("error", () => reject(new Error("turnstile script failed")), { once: true });
      return;
    }
    const script = document.createElement("script");
    script.src = "https://challenges.cloudflare.com/turnstile/v0/api.js?render=explicit";
    script.async = true;
    script.defer = true;
    script.dataset.turnstileScript = "1";
    script.onload = () => resolve();
    script.onerror = () => reject(new Error("turnstile script failed"));
    document.head.appendChild(script);
  });
  return turnstileScriptPromise;
}

function TurnstileBox({ siteKey, locale, onToken }: { siteKey: string; locale: "zh" | "en"; onToken: (token: string) => void }) {
  const { t } = useLocale();
  const [error, setError] = useState("");
  const containerId = `turnstile-${siteKey}`;

  useEffect(() => {
    if (!siteKey || typeof window === "undefined") return;
    let cancelled = false;
    let widgetId = "";
    onToken("");
    loadTurnstileScript()
      .then(() => {
        if (cancelled) return;
        const element = document.getElementById(containerId);
        const turnstile = (window as TurnstileWindow).turnstile;
        if (!element || !turnstile) return;
        element.innerHTML = "";
        widgetId = turnstile.render(element, {
          sitekey: siteKey,
          language: locale === "zh" ? "zh-cn" : "en",
          theme: "light",
          callback: (token: string) => {
            setError("");
            onToken(token);
          },
          "expired-callback": () => onToken(""),
          "error-callback": () => {
            onToken("");
            setError(t.auth.turnstileFailed);
          },
        });
      })
      .catch(() => setError(t.auth.turnstileFailed));
    return () => {
      cancelled = true;
      onToken("");
      if (widgetId && (window as TurnstileWindow).turnstile) {
        (window as TurnstileWindow).turnstile?.remove(widgetId);
      }
    };
  }, [containerId, locale, onToken, siteKey, t.auth.turnstileFailed]);

  return (
    <div className="grid gap-2">
      <div id={containerId} className="min-h-[65px]" />
      {error && <div className="text-xs font-bold text-pink-700">{error}</div>}
    </div>
  );
}

async function errorMessage(res: Response) {
  const text = await res.text();
  try {
    const body = JSON.parse(text) as { error?: { message?: string } };
    return body.error?.message || text;
  } catch {
    return text;
  }
}
