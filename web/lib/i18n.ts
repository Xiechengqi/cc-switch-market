import { copy } from "./copy";

// 兼容层：保留 messages.zh.nav / messages.zh.auth / messages.zh.common 既有结构，
// 实际数据来自 copy.ts 顶层字段，避免 t.nav.* 等调用点全量改动。
export const messages = {
  zh: {
    nav: copy.zh.nav,
    auth: copy.zh.auth,
    common: copy.zh.common,
  },
  en: {
    nav: copy.en.nav,
    auth: copy.en.auth,
    common: copy.en.common,
  },
} as const;

export type Locale = keyof typeof messages;
export type MessageTree = (typeof messages)[Locale];

export function pick<T>(locale: Locale, selector: (tree: MessageTree) => T): T {
  return selector(messages[locale]);
}
