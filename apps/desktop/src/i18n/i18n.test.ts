import { afterEach, describe, expect, it } from "vitest";
import { en } from "./en.ts";
import { setLocale, t } from "./index.ts";
import { vi } from "./vi.ts";

afterEach(() => setLocale("en"));

describe("i18n", () => {
  it("fills in parameters", () => {
    expect(t("tabs.close", { title: "Plan" })).toBe("Close Plan");
    expect(t("find.count", { current: 2, total: 9 })).toBe("2 of 9");
  });

  it("keeps an unknown parameter visible instead of printing undefined", () => {
    expect(t("tabs.close", {})).toBe("Close {title}");
  });

  it("uses Vietnamese when chosen and falls back to English for missing keys", () => {
    setLocale("vi");
    expect(document.documentElement.lang).toBe("vi");
    expect(t("sidebar.trash")).toBe(vi["sidebar.trash"]);
    expect(t("sidebar.trash")).not.toBe(en["sidebar.trash"]);
    const missing = (Object.keys(en) as (keyof typeof en)[]).find((key) => !(key in vi));
    if (missing) expect(t(missing)).toBe(en[missing]);
  });

  it("keeps the same placeholders in every Vietnamese message", () => {
    const placeholders = (text: string) => [...text.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    for (const key of Object.keys(vi) as (keyof typeof en)[]) {
      const translated = vi[key];
      expect(translated, key).toBeTruthy();
      expect(placeholders(translated ?? ""), key).toEqual(placeholders(en[key]));
    }
  });

  it("has no empty English messages", () => {
    for (const [key, message] of Object.entries(en)) expect(message.trim().length, key).toBeGreaterThan(0);
  });
});
