import { afterEach } from "vitest";
import { cleanup } from "@solidjs/testing-library";
import { setLocale } from "../i18n/index.ts";

// jsdom has no layout: give ProseMirror and the UI the few geometry hooks they probe.
const emptyRect = { x: 0, y: 0, top: 0, left: 0, right: 0, bottom: 0, width: 0, height: 0, toJSON: () => ({}) };
if (typeof Range !== "undefined") {
  Range.prototype.getBoundingClientRect = () => emptyRect as DOMRect;
  Range.prototype.getClientRects = () => ({ length: 0, item: () => null, [Symbol.iterator]: function* () {} }) as unknown as DOMRectList;
}
if (typeof Element !== "undefined") {
  Element.prototype.scrollIntoView = () => {};
}
if (typeof document !== "undefined") {
  document.elementFromPoint = () => null;
}
if (typeof window !== "undefined" && !window.matchMedia) {
  window.matchMedia = (query: string) =>
    ({
      matches: false,
      media: query,
      addEventListener: () => {},
      removeEventListener: () => {},
      addListener: () => {},
      removeListener: () => {},
      onchange: null,
      dispatchEvent: () => false,
    }) as MediaQueryList;
}

afterEach(() => {
  cleanup();
  setLocale("en");
  localStorage.clear();
  document.documentElement.removeAttribute("data-theme");
});
