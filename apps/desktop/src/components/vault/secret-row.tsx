import { Show, createEffect, createSignal, on, onCleanup, type JSX } from "solid-js";
import type { VaultSecretField } from "../../api/contract.ts";
import { toApiError } from "../../api/errors.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { ErrorBanner } from "../common/states.tsx";

const HIDE_AFTER_MS = 20_000;

export interface SecretRowProps {
  itemId: string;
  field: VaultSecretField;
  label: string;
}

/**
 * A secret that is never loaded until asked for. Reveal shows it for a short
 * time and forgets it again, as it does the moment the vault locks. Copying
 * goes through the core, which clears the clipboard later.
 */
export function SecretRow(props: SecretRowProps): JSX.Element {
  const { vault } = useApp();
  const [value, setValue] = createSignal<string | null>(null);
  const [error, setError] = createSignal<string | null>(null);
  const [busy, setBusy] = createSignal(false);
  let timer: ReturnType<typeof setTimeout> | undefined;

  const hide = () => {
    clearTimeout(timer);
    setValue(null);
  };
  onCleanup(hide);
  // Switching to another item or locking the vault wipes what was shown.
  createEffect(on(() => [props.itemId, vault.state()], hide, { defer: true }));

  const reveal = async () => {
    setBusy(true);
    setError(null);
    try {
      setValue(await vault.reveal(props.itemId, props.field));
      clearTimeout(timer);
      timer = setTimeout(hide, HIDE_AFTER_MS);
    } catch (failure) {
      const failed = toApiError(failure);
      if (failed.code !== "cancelled") setError(failed.message);
    } finally {
      setBusy(false);
    }
  };

  const copy = async () => {
    setError(null);
    try {
      await vault.copy(props.itemId, props.field);
    } catch (failure) {
      const failed = toApiError(failure);
      if (failed.code !== "cancelled") setError(failed.message);
    }
  };

  const clip = () => (vault.clipboard()?.field === props.field ? vault.clipboard() : null);

  return (
    <div class="secret-row" data-field={props.field}>
      <div class="secret-label">{props.label}</div>
      <div class="secret-value" data-multiline={props.field === "notes" ? "true" : undefined}>
        <Show when={value() !== null} fallback={<span aria-label={t("vault.hidden")}>{"•".repeat(10)}</span>}>
          <span class="secret-text" data-testid={`secret-${props.field}`}>
            {value()}
          </span>
        </Show>
      </div>
      <div class="secret-actions">
        <Show
          when={value() !== null}
          fallback={
            <Button icon="eye" busy={busy()} onClick={() => void reveal()}>
              {t("vault.reveal")}
            </Button>
          }
        >
          <Button icon="eye-off" onClick={hide}>
            {t("vault.hide")}
          </Button>
        </Show>
        <Button icon="copy" onClick={() => void copy()}>
          {t("vault.copy")}
        </Button>
      </div>
      <Show when={clip()}>
        {(countdown) => (
          <p class="clipboard-countdown" role="status" data-testid="clipboard-countdown">
            {t("vault.clipboardClears", { seconds: countdown().remaining })}
          </p>
        )}
      </Show>
      <ErrorBanner error={error()} />
    </div>
  );
}
