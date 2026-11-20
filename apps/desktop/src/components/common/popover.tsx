import { onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";

export interface PopoverProps {
  /** The element the popover hangs from, or a viewport point. */
  anchor: HTMLElement | { x: number; y: number };
  label: string;
  onClose(): void;
  children: JSX.Element;
  class?: string;
}

const FOCUSABLE = 'input:not([disabled]), button:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

/**
 * A panel anchored to an element, for editors that need more than a menu: it
 * keeps inside the window, takes focus, closes on Escape, on an outside click,
 * or when focus moves away, and gives focus back to what opened it.
 */
export function Popover(props: PopoverProps): JSX.Element {
  let panel!: HTMLDivElement;
  const opener = document.activeElement as HTMLElement | null;
  const point = () => {
    if (props.anchor instanceof HTMLElement) {
      const rect = props.anchor.getBoundingClientRect();
      return { x: rect.left, y: rect.bottom + 4 };
    }
    return props.anchor;
  };

  onMount(() => {
    const rect = panel.getBoundingClientRect();
    const { x, y } = point();
    panel.style.left = `${Math.max(8, Math.min(x, window.innerWidth - rect.width - 8))}px`;
    panel.style.top = `${Math.max(8, Math.min(y, window.innerHeight - rect.height - 8))}px`;
    (panel.querySelector<HTMLElement>("[data-autofocus]") ?? panel.querySelector<HTMLElement>(FOCUSABLE) ?? panel).focus();

    const outside = (event: PointerEvent) => {
      const target = event.target as Node;
      if (!panel.contains(target) && !(props.anchor instanceof HTMLElement && props.anchor.contains(target))) props.onClose();
    };
    document.addEventListener("pointerdown", outside, true);
    window.addEventListener("blur", props.onClose);
    onCleanup(() => {
      document.removeEventListener("pointerdown", outside, true);
      window.removeEventListener("blur", props.onClose);
      if (opener?.isConnected) opener.focus?.();
    });
  });

  return (
    <Portal>
      <div
        ref={panel}
        class={`popover ${props.class ?? ""}`}
        role="dialog"
        aria-label={props.label}
        tabindex="-1"
        style={{ left: `${point().x}px`, top: `${point().y}px` }}
        onKeyDown={(event) => {
          if (event.key !== "Escape") return;
          event.preventDefault();
          event.stopPropagation();
          props.onClose();
        }}
        onFocusOut={(event) => {
          const next = event.relatedTarget as Node | null;
          if (next && !panel.contains(next)) props.onClose();
        }}
      >
        {props.children}
      </div>
    </Portal>
  );
}
