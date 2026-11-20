import { describe, expect, it } from "vitest";
import { findAll, fold, matchesQuery } from "./text.ts";
import { splitHighlights } from "./highlight.ts";
import { MIN_PASSWORD_LENGTH, passwordStrength } from "./password.ts";
import { acceleratorFromEvent, describeAccelerator } from "./hotkey.ts";
import { isSafeHref, normalizeHref } from "../editor/links.ts";
import { encodeArgs, decodeBytes, toSnakeCase } from "../api/tauri.ts";

describe("text folding", () => {
  it("ignores case and Vietnamese diacritics", () => {
    expect(fold("Ghi chú")).toBe("ghi chu");
    expect(fold("Đường")).toBe("duong");
    expect(matchesQuery("Ghi chú họp", "ghi chu")).toBe(true);
    expect(matchesQuery("Ghi chú họp", "hop ghi")).toBe(true);
    expect(matchesQuery("Ghi chú họp", "xyz")).toBe(false);
  });

  it("keeps offsets valid", () => {
    expect(fold("Đà Nẵng").length).toBe("Đà Nẵng".length);
    expect(findAll("Chào chào", "chao")).toEqual([0, 5]);
  });
});

describe("highlight runs", () => {
  it("marks and merges ranges", () => {
    expect(splitHighlights("abcdef", [[1, 3], [2, 4]])).toEqual([
      { text: "a", mark: false },
      { text: "bcd", mark: true },
      { text: "ef", mark: false },
    ]);
  });
  it("clamps ranges outside the text and ignores empty ones", () => {
    expect(splitHighlights("abc", [[-5, 1], [2, 99], [1, 1]])).toEqual([
      { text: "a", mark: true },
      { text: "b", mark: false },
      { text: "c", mark: true },
    ]);
    expect(splitHighlights("abc", [])).toEqual([{ text: "abc", mark: false }]);
  });
});

describe("password strength", () => {
  it("requires the minimum length", () => {
    expect(passwordStrength("a".repeat(MIN_PASSWORD_LENGTH - 1)).level).toBe("tooShort");
  });
  it("rates repeated and sequential passwords low and long mixed ones high", () => {
    expect(passwordStrength("aaaaaaaaaaaa").score).toBeLessThanOrEqual(1);
    expect(passwordStrength("abcdefghijkl").score).toBeLessThanOrEqual(1);
    expect(passwordStrength("t7#Vq!x9Lm@2Zp4wRk").score).toBe(4);
  });
});

describe("hotkeys", () => {
  const press = (init: Partial<KeyboardEvent> & { code: string; key: string }) => ({ ctrlKey: false, metaKey: false, altKey: false, shiftKey: false, ...init });
  it("builds accelerators and needs a modifier", () => {
    expect(acceleratorFromEvent(press({ code: "Space", key: " ", metaKey: true, shiftKey: true }), true)).toBe("CmdOrCtrl+Shift+Space");
    expect(acceleratorFromEvent(press({ code: "KeyK", key: "k", ctrlKey: true }), false)).toBe("CmdOrCtrl+K");
    expect(acceleratorFromEvent(press({ code: "KeyK", key: "k" }), false)).toBeNull();
    expect(acceleratorFromEvent(press({ code: "ShiftLeft", key: "Shift", shiftKey: true }), true)).toBeNull();
  });
  it("describes them per platform", () => {
    expect(describeAccelerator("CmdOrCtrl+Shift+Space", true)).toBe("⌘⇧Space");
    expect(describeAccelerator("CmdOrCtrl+Shift+Space", false)).toBe("Ctrl+Shift+Space");
  });
});

describe("link safety", () => {
  it("normalises what people type", () => {
    expect(normalizeHref("example.com/a")).toBe("https://example.com/a");
    expect(normalizeHref("http://example.com")).toBe("http://example.com");
    expect(normalizeHref("me@example.com")).toBe("mailto:me@example.com");
  });
  it("refuses schemes that run code or read files", () => {
    expect(normalizeHref("javascript:alert(1)")).toBeNull();
    expect(normalizeHref("JavaScript:alert(1)")).toBeNull();
    expect(normalizeHref("data:text/html,x")).toBeNull();
    expect(normalizeHref("file:///etc/passwd")).toBeNull();
    expect(normalizeHref("two words")).toBeNull();
    expect(normalizeHref("  ")).toBeNull();
    expect(isSafeHref("javascript:alert(1)")).toBe(false);
    expect(isSafeHref("https://example.com")).toBe(true);
    expect(isSafeHref("ne-page:abc")).toBe(true);
  });
});

describe("Tauri argument encoding", () => {
  it("maps camelCase commands to snake_case", () => {
    expect(toSnakeCase("treeMove")).toBe("tree_move");
    expect(toSnakeCase("vaultGeneratePassword")).toBe("vault_generate_password");
    expect(toSnakeCase("appVersion")).toBe("app_version");
  });
  it("turns nested binary into plain byte arrays", () => {
    expect(encodeArgs({ id: "p", update: new Uint8Array([1, 2, 3]), nested: [{ bytes: new Uint8Array([9]) }] })).toEqual({
      id: "p",
      update: [1, 2, 3],
      nested: [{ bytes: [9] }],
    });
  });
  it("reads every binary shape the core may return", () => {
    expect([...decodeBytes([1, 2])]).toEqual([1, 2]);
    expect([...decodeBytes(new Uint8Array([3]).buffer)]).toEqual([3]);
    expect([...decodeBytes(new Uint8Array([4]))]).toEqual([4]);
    expect(() => decodeBytes("nope")).toThrow();
  });
});
