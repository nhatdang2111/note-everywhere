import { createUniqueId, onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { t } from "../../i18n/index.ts";
import { IconButton } from "./button.tsx";

const FOCUSABLE = 'a[href], button:not([disabled]), input:not([disabled]), select:not([disabled]), textarea:not([disabled]), [tabindex]:not([tabindex="-1"])';

export interface DialogProps {
  title: string;
  onClose(): void;
  children: JSX.Element;
  /** Buttons row below the content. */
  footer?: JSX.Element;
  /** Clicking the backdrop closes the dialog. On by default; turn off for forms that hold input. */
  dismissOnBackdrop?: boolean;
  /** Hide the close button, for steps that must be completed (for example saving a recovery key). */
  hideClose?: boolean;
  wide?: boolean;
}

/** Modal dialog: traps focus, closes on Escape, and gives focus back to what opened it. */
export function Dialog(props: DialogProps): JSX.Element {
  const titleId = createUniqueId();
  let panel!: HTMLDivElement;
  const opener = document.activeElement as HTMLElement | null;

  onMount(() => {
    const first = panel.querySelector<HTMLElement>("[data-autofocus]") ?? panel.querySelector<HTMLElement>(FOCUSABLE);
    (first ?? panel).focus();
  });
  onCleanup(() => opener?.focus?.());

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "Escape" && !props.hideClose) {
      event.stopPropagation();
      props.onClose();
      return;
    }
    if (event.key !== "Tab") return;
    const items = [...panel.querySelectorAll<HTMLElement>(FOCUSABLE)];
    const first = items[0];
    const last = items[items.length - 1];
    if (!first || !last) return event.preventDefault();
    if (event.shiftKey && document.activeElement === first) (event.preventDefault(), last.focus());
    else if (!event.shiftKey && document.activeElement === last) (event.preventDefault(), first.focus());
  };

  return (
    <Portal>
      <div
        class="dialog-backdrop"
        onMouseDown={(event) => {
          if (event.target === event.currentTarget && props.dismissOnBackdrop !== false && !props.hideClose) props.onClose();
        }}
      >
        <div
          ref={panel}
          class={`dialog ${props.wide ? "dialog-wide" : ""}`}
          role="dialog"
          aria-modal="true"
          aria-labelledby={titleId}
          tabindex="-1"
          onKeyDown={onKeyDown}
        >
          <header class="dialog-header">
            <h2 id={titleId}>{props.title}</h2>
            {!props.hideClose && <IconButton icon="close" label={t("common.close")} onClick={() => props.onClose()} />}
          </header>
          <div class="dialog-body">{props.children}</div>
          {props.footer && <footer class="dialog-footer">{props.footer}</footer>}
        </div>
      </div>
    </Portal>
  );
}
