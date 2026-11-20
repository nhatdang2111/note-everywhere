import { createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { Button } from "../common/button.tsx";
import { Checkbox } from "../common/form.tsx";

export interface RecoveryKeyPanelProps {
  recoveryKey: string;
  /** What the key recovers, for example "account" or "vault". */
  subject: "account" | "vault";
  onConfirm(): void;
}

/**
 * Shows a recovery key once and will not continue until the person says they
 * saved it. The key is rendered as text only; it is never written to the
 * console or to storage by the UI.
 */
export function RecoveryKeyPanel(props: RecoveryKeyPanelProps): JSX.Element {
  const [saved, setSaved] = createSignal(false);
  const [copied, setCopied] = createSignal(false);

  const copy = async () => {
    try {
      await navigator.clipboard.writeText(props.recoveryKey);
      setCopied(true);
    } catch {
      setCopied(false);
    }
  };

  return (
    <div class="recovery-panel">
      <p>{props.subject === "account" ? t("recovery.accountIntro") : t("recovery.vaultIntro")}</p>
      <p class="recovery-warning">{t("recovery.warning")}</p>
      <pre class="recovery-key" aria-label={t("recovery.keyLabel")} data-testid="recovery-key">
        {props.recoveryKey}
      </pre>
      <div class="recovery-actions">
        <Button icon="copy" onClick={() => void copy()}>
          {copied() ? t("recovery.copied") : t("recovery.copy")}
        </Button>
      </div>
      <Checkbox label={t("recovery.confirm")} checked={saved()} onChange={setSaved} />
      <Button variant="primary" disabled={!saved()} onClick={() => props.onConfirm()}>
        {t("recovery.continue")}
      </Button>
    </div>
  );
}
