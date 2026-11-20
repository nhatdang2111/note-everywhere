import { Show, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { MIN_PASSWORD_LENGTH } from "../../lib/password.ts";
import { createSubmit, invalid } from "../../lib/use-submit.ts";
import { useApp } from "../../state/app.tsx";
import { PasswordStrengthMeter } from "../auth/password-strength-meter.tsx";
import { Button } from "../common/button.tsx";
import { PasswordField, TextField } from "../common/form.tsx";
import { ErrorBanner } from "../common/states.tsx";

/** First use: choose the second password. */
export function VaultSetup(props: { onCreated(sameAsAccountPassword: boolean): void }): JSX.Element {
  const { vault } = useApp();
  const [password, setPassword] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const form = createSubmit(async () => {
    if (password().length < MIN_PASSWORD_LENGTH) throw invalid(t("auth.passwordTooShort", { min: MIN_PASSWORD_LENGTH }));
    if (password() !== confirm()) throw invalid(t("auth.passwordsDiffer"));
    const { sameAsAccountPassword } = await vault.create(password());
    props.onCreated(sameAsAccountPassword);
  });
  return (
    <form class="form vault-gate" onSubmit={form.submit} aria-label={t("vault.setupTitle")}>
      <h1>{t("vault.setupTitle")}</h1>
      <p>{t("vault.setupIntro")}</p>
      <PasswordField label={t("vault.password")} value={password()} onInput={setPassword} autocomplete="new-password" autofocus />
      <PasswordStrengthMeter password={password()} />
      <PasswordField label={t("auth.confirmPassword")} value={confirm()} onInput={setConfirm} autocomplete="new-password" />
      <ErrorBanner error={form.error()} />
      <div class="form-actions">
        <Button variant="primary" type="submit" busy={form.busy()}>
          {t("vault.create")}
        </Button>
      </div>
    </form>
  );
}

/** The vault is locked: nothing but a password box and the way out through the recovery key. */
export function VaultUnlock(): JSX.Element {
  const { vault } = useApp();
  const [recovering, setRecovering] = createSignal(false);
  const [password, setPassword] = createSignal("");
  const [recoveryKey, setRecoveryKey] = createSignal("");
  const [newPassword, setNewPassword] = createSignal("");

  const unlock = createSubmit(async () => {
    if (!password()) throw invalid(t("vault.enterPassword"));
    await vault.unlock(password());
    setPassword("");
  });
  const recover = createSubmit(async () => {
    if (!recoveryKey().trim()) throw invalid(t("auth.fillAll"));
    if (newPassword().length < MIN_PASSWORD_LENGTH) throw invalid(t("auth.passwordTooShort", { min: MIN_PASSWORD_LENGTH }));
    await vault.recover(recoveryKey().trim(), newPassword());
    setRecoveryKey("");
    setNewPassword("");
  });

  return (
    <Show
      when={!recovering()}
      fallback={
        <form class="form vault-gate" onSubmit={recover.submit} aria-label={t("vault.recoverTitle")}>
          <h1>{t("vault.recoverTitle")}</h1>
          <p>{t("vault.recoverIntro")}</p>
          <TextField label={t("auth.recoveryKey")} value={recoveryKey()} onInput={setRecoveryKey} spellcheck={false} autocomplete="off" autofocus />
          <PasswordField label={t("auth.newPassword")} value={newPassword()} onInput={setNewPassword} autocomplete="new-password" />
          <PasswordStrengthMeter password={newPassword()} />
          <ErrorBanner error={recover.error()} />
          <div class="form-actions">
            <Button onClick={() => setRecovering(false)}>{t("common.back")}</Button>
            <Button variant="primary" type="submit" busy={recover.busy()}>
              {t("vault.recover")}
            </Button>
          </div>
        </form>
      }
    >
      <form class="form vault-gate" onSubmit={unlock.submit} aria-label={t("vault.lockedTitle")}>
        <h1>{t("vault.lockedTitle")}</h1>
        <p>{t("vault.lockedIntro")}</p>
        <PasswordField label={t("vault.password")} value={password()} onInput={setPassword} autocomplete="off" autofocus />
        <ErrorBanner error={unlock.error()} />
        <div class="form-actions">
          <Button variant="primary" type="submit" busy={unlock.busy()} icon="unlock">
            {t("vault.unlock")}
          </Button>
        </div>
        <Button class="link-button" variant="ghost" onClick={() => setRecovering(true)}>
          {t("vault.forgot")}
        </Button>
      </form>
    </Show>
  );
}
