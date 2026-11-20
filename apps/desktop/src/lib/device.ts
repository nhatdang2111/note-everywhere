export const isMac = (): boolean => typeof navigator !== "undefined" && /Mac|iPhone|iPad/i.test(navigator.platform || navigator.userAgent);

/** Label for the modifier key in shortcut hints. */
export const modLabel = (): string => (isMac() ? "⌘" : "Ctrl");

/** A default name for this device in the sessions list, which the person can change. */
export function defaultDeviceLabel(): string {
  if (typeof navigator === "undefined") return "This device";
  if (/Mac/i.test(navigator.platform || navigator.userAgent)) return "Mac";
  if (/Win/i.test(navigator.platform || navigator.userAgent)) return "Windows PC";
  return "Desktop";
}
