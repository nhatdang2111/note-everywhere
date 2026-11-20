const MODIFIER_KEYS = new Set(["Control", "Shift", "Alt", "Meta", "AltGraph"]);

function keyName(code: string, key: string): string | null {
  if (/^Key[A-Z]$/.test(code)) return code.slice(3);
  if (/^Digit\d$/.test(code)) return code.slice(5);
  if (/^F\d{1,2}$/.test(code)) return code;
  const named: Record<string, string> = {
    Space: "Space",
    Enter: "Enter",
    Tab: "Tab",
    Backspace: "Backspace",
    ArrowUp: "Up",
    ArrowDown: "Down",
    ArrowLeft: "Left",
    ArrowRight: "Right",
    Minus: "-",
    Equal: "=",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Semicolon: ";",
    Quote: "'",
    Backquote: "`",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
  };
  return named[code] ?? (key.length === 1 ? key.toUpperCase() : null);
}

/**
 * Turn a key press into an accelerator string such as `CmdOrCtrl+Shift+Space`.
 * Returns null while only modifiers are held or the key has no name; a hotkey
 * needs at least one modifier so it cannot swallow ordinary typing.
 */
export function acceleratorFromEvent(event: Pick<KeyboardEvent, "code" | "key" | "ctrlKey" | "metaKey" | "altKey" | "shiftKey">, mac: boolean): string | null {
  if (MODIFIER_KEYS.has(event.key)) return null;
  const name = keyName(event.code, event.key);
  if (!name) return null;
  const parts: string[] = [];
  if (mac ? event.metaKey : event.ctrlKey) parts.push("CmdOrCtrl");
  if (mac && event.ctrlKey) parts.push("Ctrl");
  if (!mac && event.metaKey) parts.push("Super");
  if (event.altKey) parts.push("Alt");
  if (event.shiftKey) parts.push("Shift");
  if (parts.length === 0) return null;
  return [...parts, name].join("+");
}

/** Human-readable form of an accelerator for the current platform. */
export function describeAccelerator(accelerator: string, mac: boolean): string {
  return accelerator
    .split("+")
    .map((part) => {
      if (part === "CmdOrCtrl") return mac ? "⌘" : "Ctrl";
      if (part === "Alt") return mac ? "⌥" : "Alt";
      if (part === "Shift") return mac ? "⇧" : "Shift";
      if (part === "Ctrl") return mac ? "⌃" : "Ctrl";
      return part;
    })
    .join(mac ? "" : "+");
}
