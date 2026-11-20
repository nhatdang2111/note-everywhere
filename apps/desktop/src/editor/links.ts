import { PAGE_HREF_PREFIX } from "./page-links.ts";

const SAFE_SCHEMES = ["http:", "https:", "mailto:", "tel:"];

function schemeOf(value: string): string | null {
  const match = /^([a-z][a-z0-9+.-]*):/i.exec(value);
  return match ? `${(match[1] ?? "").toLowerCase()}:` : null;
}

/** True when opening `href` is safe: web, mail and phone links and links between pages. */
export function isSafeHref(href: string): boolean {
  if (href.startsWith(PAGE_HREF_PREFIX)) return true;
  const scheme = schemeOf(href.trim());
  return scheme !== null && SAFE_SCHEMES.includes(scheme);
}

/**
 * Turn what someone typed into a link target. Bare domains get `https://`;
 * schemes that can run code or load local files (`javascript:`, `data:`,
 * `file:`) are refused, as is anything empty. Returns null when refused.
 */
export function normalizeHref(raw: string): string | null {
  const value = raw.trim();
  if (!value || /\s/.test(value)) return null;
  const scheme = schemeOf(value);
  if (scheme) return SAFE_SCHEMES.includes(scheme) ? value : null;
  if (value.includes("@") && !value.includes("/")) return `mailto:${value}`;
  return `https://${value}`;
}
