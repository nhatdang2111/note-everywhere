import { For, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { IconButton } from "./button.tsx";
import { Icon } from "./icons.tsx";

/** Transient messages. Errors are announced assertively, the rest politely. */
export function ToastHost(): JSX.Element {
  const { toasts } = useApp();
  return (
    <div class="toast-host" aria-live="polite">
      <For each={toasts.items()}>
        {(toast) => (
          <div class={`toast toast-${toast.kind}`} role={toast.kind === "error" ? "alert" : "status"}>
            <Icon name={toast.kind === "error" ? "alert" : toast.kind === "success" ? "check" : "info"} />
            <span class="toast-message">{toast.message}</span>
            <IconButton icon="close" label={t("common.dismiss")} size={14} onClick={() => toasts.dismiss(toast.id)} />
          </div>
        )}
      </For>
    </div>
  );
}
