import { createSignal } from "solid-js";
import { en, type MessageKey } from "./en.ts";
import { vi } from "./vi.ts";

export type Locale = "en" | "vi";
export type { MessageKey };

export const LOCALES: { id: Locale; label: string }[] = [
  { id: "en", label: "English" },
  { id: "vi", label: "Tiếng Việt" },
];

const catalogues: Record<Locale, Partial<Record<MessageKey, string>>> = { en, vi };
const STORAGE_KEY = "ne.locale";

function initialLocale(): Locale {
  const stored = typeof localStorage === "undefined" ? null : localStorage.getItem(STORAGE_KEY);
  if (stored === "en" || stored === "vi") return stored;
  return typeof navigator !== "undefined" && navigator.language.toLowerCase().startsWith("vi") ? "vi" : "en";
}

const [locale, setLocaleSignal] = createSignal<Locale>(initialLocale());

export { locale };

export function setLocale(next: Locale): void {
  localStorage.setItem(STORAGE_KEY, next);
  setLocaleSignal(next);
  document.documentElement.lang = next;
}

export type MessageParams = Record<string, string | number>;

/** Look up a message. Reads the locale signal, so it is reactive inside JSX and effects. */
export function t(key: MessageKey, params?: MessageParams): string {
  const message = catalogues[locale()][key] ?? en[key];
  if (!params) return message;
  return message.replace(/\{(\w+)\}/g, (_, name: string) => String(params[name] ?? `{${name}}`));
}
