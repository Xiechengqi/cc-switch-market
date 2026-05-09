import type { Metadata } from "next";
import { MarketAuthProvider } from "@/components/auth";
import { LanguageProvider } from "@/components/language-provider";
import { ToastProvider } from "@/components/ui/Toast";
import { LegacyRedirect } from "@/components/legacy-redirect";
import { TooltipProvider } from "@/components/shadcn/tooltip";
import { BRAND } from "@/lib/copy";
import "./globals.css";

export const metadata: Metadata = {
  title: BRAND.zh.pageTitle,
  description: BRAND.zh.metaDescription
};

export default function RootLayout({ children }: { children: React.ReactNode }) {
  return (
    <html lang="zh-CN">
      <body>
        <TooltipProvider>
          <ToastProvider>
            <LanguageProvider>
              <MarketAuthProvider>
                <LegacyRedirect />
                {children}
              </MarketAuthProvider>
            </LanguageProvider>
          </ToastProvider>
        </TooltipProvider>
      </body>
    </html>
  );
}
