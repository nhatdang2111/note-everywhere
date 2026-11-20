import { For, onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { Icon, type IconName } from "./icons.tsx";

export type MenuEntry =
  | {
      kind: "item";
      label: string;
      icon?: IconName;
      danger?: boolean;
      disabled?: boolean;
      /** Shows a check mark, for a choice that is currently selected. */
      checked?: boolean;
      onSelect(): void;
    }
  | { kind: "separator" }
  | { kind: "heading"; label: string };

export interface MenuProps {
  entries: MenuEntry[];
  /** Viewport position of the menu's top-left corner. */
  at: { x: number; y: number };
  label: string;
  onClose(): void;
}

function renderEntry(entry: MenuEntry, close: () => void): JSX.Element {
  if (entry.kind === "separator") return <div class="menu-separator" role="separator" />;
  if (entry.kind === "heading") return <div class="menu-heading">{entry.label}</div>;
  return (
    <button
      type="button"
      role="menuitem"
      class={`menu-item ${entry.danger ? "menu-item-danger" : ""}`}
      disabled={entry.disabled}
      onClick={() => {
        close();
        entry.onSelect();
      }}
    >
      <span class="menu-item-icon">{entry.icon && <Icon name={entry.icon} />}</span>
      <span class="menu-item-label">{entry.label}</span>
      {entry.checked && <Icon name="check" />}
    </button>
  );
}

/** Popup menu: arrow keys move, Enter selects, Escape or an outside click closes. */
export function Menu(props: MenuProps): JSX.Element {
  let panel!: HTMLDivElement;
  const opener = document.activeElement as HTMLElement | null;

  const items = () => [...panel.querySelectorAll<HTMLButtonElement>('[role="menuitem"]:not([disabled])')];

  onMount(() => {
    // Keep the menu inside the window.
    const rect = panel.getBoundingClientRect();
    const left = Math.max(8, Math.min(props.at.x, window.innerWidth - rect.width - 8));
    const top = Math.max(8, Math.min(props.at.y, window.innerHeight - rect.height - 8));
    panel.style.left = `${left}px`;
    panel.style.top = `${top}px`;
    items()[0]?.focus();

    const outside = (event: PointerEvent) => {
      if (!panel.contains(event.target as Node)) props.onClose();
    };
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("blur", props.onClose);
    onCleanup(() => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("blur", props.onClose);
      opener?.focus?.();
    });
  });

  const onKeyDown = (event: KeyboardEvent) => {
    const list = items();
    const at = list.indexOf(document.activeElement as HTMLButtonElement);
    const move = (to: number) => {
      event.preventDefault();
      list[(to + list.length) % list.length]?.focus();
    };
    switch (event.key) {
      case "ArrowDown":
        return move(at + 1);
      case "ArrowUp":
        return move(at <= 0 ? list.length - 1 : at - 1);
      case "Home":
        return move(0);
      case "End":
        return move(list.length - 1);
      case "Escape":
      case "Tab":
        event.preventDefault();
        event.stopPropagation();
        return props.onClose();
    }
  };

  return (
    <Portal>
      <div ref={panel} class="menu" role="menu" aria-label={props.label} style={{ left: `${props.at.x}px`, top: `${props.at.y}px` }} onKeyDown={onKeyDown}>
        <For each={props.entries}>{(entry) => renderEntry(entry, props.onClose)}</For>
      </div>
    </Portal>
  );
}
