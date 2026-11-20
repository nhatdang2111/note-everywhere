import { createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { defaultDeviceLabel } from "../../lib/device.ts";
import { MIN_PASSWORD_LENGTH } from "../../lib/password.ts";
import { createSubmit, invalid } from "../../lib/use-submit.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { PasswordField, TextField } from "../common/form.tsx";
import { ErrorBanner } from "../common/states.tsx";
import { PasswordStrengthMeter } from "./password-strength-meter.tsx";

export interface AccountFormProps {
  /** Prefilled values, for "password needed again". */
  serverUrl?: string;
  username?: string;
  onBack?(): void;
}

const filled = (...values: string[]) => values.every((value) => value.trim().length > 0);

function checkNewPassword(password: string, confirm: string): void {
  if (password.length < MIN_PASSWORD_LENGTH) throw invalid(t("auth.passwordTooShort", { min: MIN_PASSWORD_LENGTH }));
  if (password !== confirm) throw invalid(t("auth.passwordsDiffer"));
}

function Actions(props: { busy: boolean; label: string; onBack?: () => void }): JSX.Element {
  return (
    <div class="form-actions">
      {props.onBack && <Button onClick={() => props.onBack?.()}>{t("common.back")}</Button>}
      <Button variant="primary" type="submit" busy={props.busy}>
        {props.label}
      </Button>
    </div>
  );
}

export function SignUpForm(props: AccountFormProps): JSX.Element {
  const { session } = useApp();
  const [serverUrl, setServerUrl] = createSignal(props.serverUrl ?? "");
  const [setupCode, setSetupCode] = createSignal("");
  const [username, setUsername] = createSignal(props.username ?? "");
  const [password, setPassword] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const [deviceLabel, setDeviceLabel] = createSignal(defaultDeviceLabel());

  const form = createSubmit(async () => {
    if (!filled(serverUrl(), setupCode(), username(), deviceLabel())) throw invalid(t("auth.fillAll"));
    checkNewPassword(password(), confirm());
    await session.signUp({
      serverUrl: serverUrl().trim(),
      setupCode: setupCode().trim(),
      username: username().trim(),
      password: password(),
      deviceLabel: deviceLabel().trim(),
    });
  });

  return (
    <form class="form" onSubmit={form.submit} aria-label={t("auth.signUp")}>
      <TextField label={t("auth.serverUrl")} type="url" value={serverUrl()} onInput={setServerUrl} placeholder="https://notes.example.workers.dev" autocomplete="url" autofocus />
      <TextField label={t("auth.setupCode")} value={setupCode()} onInput={setSetupCode} hint={t("auth.setupCodeHint")} autocomplete="off" />
      <TextField label={t("auth.username")} value={username()} onInput={setUsername} autocomplete="username" />
      <PasswordField label={t("auth.password")} value={password()} onInput={setPassword} autocomplete="new-password" />
      <PasswordStrengthMeter password={password()} />
      <PasswordField label={t("auth.confirmPassword")} value={confirm()} onInput={setConfirm} autocomplete="new-password" />
      <TextField label={t("auth.deviceLabel")} value={deviceLabel()} onInput={setDeviceLabel} hint={t("auth.deviceLabelHint")} />
      <ErrorBanner error={form.error()} />
      <Actions busy={form.busy()} label={t("auth.createAccount")} onBack={props.onBack} />
    </form>
  );
}

export function SignInForm(props: AccountFormProps): JSX.Element {
  const { session } = useApp();
  const [serverUrl, setServerUrl] = createSignal(props.serverUrl ?? "");
  const [username, setUsername] = createSignal(props.username ?? "");
  const [password, setPassword] = createSignal("");
  const [deviceLabel, setDeviceLabel] = createSignal(defaultDeviceLabel());

  const form = createSubmit(async () => {
    if (!filled(serverUrl(), username(), deviceLabel()) || password() === "") throw invalid(t("auth.fillAll"));
    await session.signIn({ serverUrl: serverUrl().trim(), username: username().trim(), password: password(), deviceLabel: deviceLabel().trim() });
  });

  return (
    <form class="form" onSubmit={form.submit} aria-label={t("auth.signIn")}>
      <TextField label={t("auth.serverUrl")} type="url" value={serverUrl()} onInput={setServerUrl} autocomplete="url" autofocus={!props.serverUrl} />
      <TextField label={t("auth.username")} value={username()} onInput={setUsername} autocomplete="username" />
      <PasswordField label={t("auth.password")} value={password()} onInput={setPassword} autocomplete="current-password" autofocus={Boolean(props.serverUrl)} />
      <TextField label={t("auth.deviceLabel")} value={deviceLabel()} onInput={setDeviceLabel} />
      <ErrorBanner error={form.error()} />
      <Actions busy={form.busy()} label={t("auth.signIn")} onBack={props.onBack} />
    </form>
  );
}

export function RecoverForm(props: AccountFormProps): JSX.Element {
  const { session } = useApp();
  const [serverUrl, setServerUrl] = createSignal(props.serverUrl ?? "");
  const [username, setUsername] = createSignal(props.username ?? "");
  const [recoveryKey, setRecoveryKey] = createSignal("");
  const [password, setPassword] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const [deviceLabel, setDeviceLabel] = createSignal(defaultDeviceLabel());

  const form = createSubmit(async () => {
    if (!filled(serverUrl(), username(), recoveryKey(), deviceLabel())) throw invalid(t("auth.fillAll"));
    checkNewPassword(password(), confirm());
    await session.recover({
      serverUrl: serverUrl().trim(),
      username: username().trim(),
      recoveryKey: recoveryKey().trim(),
      newPassword: password(),
      deviceLabel: deviceLabel().trim(),
    });
  });

  return (
    <form class="form" onSubmit={form.submit} aria-label={t("auth.recover")}>
      <p class="form-intro">{t("auth.recoverIntro")}</p>
      <TextField label={t("auth.serverUrl")} type="url" value={serverUrl()} onInput={setServerUrl} autocomplete="url" />
      <TextField label={t("auth.username")} value={username()} onInput={setUsername} autocomplete="username" />
      <TextField label={t("auth.recoveryKey")} value={recoveryKey()} onInput={setRecoveryKey} spellcheck={false} autocomplete="off" autofocus />
      <PasswordField label={t("auth.newPassword")} value={password()} onInput={setPassword} autocomplete="new-password" />
      <PasswordStrengthMeter password={password()} />
      <PasswordField label={t("auth.confirmPassword")} value={confirm()} onInput={setConfirm} autocomplete="new-password" />
      <TextField label={t("auth.deviceLabel")} value={deviceLabel()} onInput={setDeviceLabel} />
      <ErrorBanner error={form.error()} />
      <Actions busy={form.busy()} label={t("auth.recoverAndSignIn")} onBack={props.onBack} />
    </form>
  );
}
