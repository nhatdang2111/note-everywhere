import { locale } from "../i18n/index.ts";

/** Short date and time for a millisecond timestamp, in the active language. */
export function formatDateTime(ms: number): string {
  return new Intl.DateTimeFormat(locale(), { dateStyle: "medium", timeStyle: "short" }).format(new Date(ms));
}

/** "3 minutes ago" style text, falling back to the date for old values. */
export function formatRelative(ms: number, now = Date.now()): string {
  const seconds = Math.round((ms - now) / 1000);
  const abs = Math.abs(seconds);
  const format = new Intl.RelativeTimeFormat(locale(), { numeric: "auto" });
  if (abs < 45) return format.format(0, "second");
  if (abs < 3600) return format.format(Math.round(seconds / 60), "minute");
  if (abs < 86_400) return format.format(Math.round(seconds / 3600), "hour");
  if (abs < 86_400 * 14) return format.format(Math.round(seconds / 86_400), "day");
  return formatDateTime(ms);
}

export function formatBytes(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB"];
  let value = bytes / 1024;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) (value /= 1024), (unit += 1);
  return `${value.toFixed(value >= 100 ? 0 : 1)} ${units[unit]}`;
}
