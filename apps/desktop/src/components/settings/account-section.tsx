import { For, Match, Show, Switch, createResource, createSignal, type JSX } from "solid-js";
import type { DeviceSession } from "../../api/contract.ts";
import { toApiError } from "../../api/errors.ts";
import { t } from "../../i18n/index.ts";
import { formatDateTime, formatRelative } from "../../lib/format.ts";
import { MIN_PASSWORD_LENGTH } from "../../lib/password.ts";
import { createSubmit, invalid } from "../../lib/use-submit.ts";
import { useApp } from "../../state/app.tsx";
import { PasswordStrengthMeter } from "../auth/password-strength-meter.tsx";
import { AccountFlow } from "../auth/auth-screens.tsx";
import { Button } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";
import { Dialog } from "../common/dialog.tsx";
import { PasswordField } from "../common/form.tsx";
import { ErrorBanner, Spinner } from "../common/states.tsx";

function ChangePasswordForm(): JSX.Element {
  const { core, toasts } = useApp();
  const [current, setCurrent] = createSignal("");
  const [next, setNext] = createSignal("");
  const [confirm, setConfirm] = createSignal("");
  const form = createSubmit(async () => {
    if (!current()) throw invalid(t("auth.fillAll"));
    if (next().length < MIN_PASSWORD_LENGTH) throw invalid(t("auth.passwordTooShort", { min: MIN_PASSWORD_LENGTH }));
    if (next() !== confirm()) throw invalid(t("auth.passwordsDiffer"));
    await core.api.changePassword({ currentPassword: current(), newPassword: next() });
    setCurrent("");
    setNext("");
    setConfirm("");
    toasts.push("success", t("settings.passwordChanged"));
  });
  return (
    <form class="form" onSubmit={form.submit} aria-label={t("settings.changePassword")}>
      <h3>{t("settings.changePassword")}</h3>
      <PasswordField label={t("vault.currentPassword")} value={current()} onInput={setCurrent} autocomplete="current-password" />
      <PasswordField label={t("auth.newPassword")} value={next()} onInput={setNext} autocomplete="new-password" />
      <PasswordStrengthMeter password={next()} />
      <PasswordField label={t("auth.confirmPassword")} value={confirm()} onInput={setConfirm} autocomplete="new-password" />
      <p class="field-hint">{t("settings.changePasswordHint")}</p>
      <ErrorBanner error={form.error()} />
      <div class="form-actions">
        <Button variant="primary" type="submit" busy={form.busy()}>
          {t("settings.changePassword")}
        </Button>
      </div>
    </form>
  );
}

function DeviceList(): JSX.Element {
  const { core, toasts } = useApp();
  const [devices, { refetch }] = createResource(() => core.api.sessionsList());
  const [revoking, setRevoking] = createSignal<DeviceSession | null>(null);

  const revoke = async () => {
    const device = revoking();
    setRevoking(null);
    if (!device) return;
    try {
      await core.api.sessionsRevoke({ id: device.id });
      toasts.push("success", t("settings.deviceRevoked", { label: device.deviceLabel }));
      await refetch();
    } catch (error) {
      toasts.error(error);
    }
  };

  return (
    <div class="devices">
      <h3>{t("settings.devices")}</h3>
      <p class="view-intro">{t("settings.devicesHint")}</p>
      <Show when={!devices.error} fallback={<ErrorBanner error={toApiError(devices.error)} />}>
        <Show when={devices()} fallback={<Spinner />}>
          {(list) => (
            <ul class="device-list">
              <For each={list()}>
                {(device) => (
                  <li class="device-row">
                    <span class="device-name">
                      {device.deviceLabel}
                      <Show when={device.current}>
                        <span class="badge" data-active="true">
                          {t("settings.thisDevice")}
                        </span>
                      </Show>
                    </span>
                    <span class="muted" title={formatDateTime(device.lastSeenAt)}>
                      {t("settings.lastSeen", { when: formatRelative(device.lastSeenAt) })}
                    </span>
                    <Show when={!device.current}>
                      <Button variant="danger" onClick={() => setRevoking(device)}>
                        {t("settings.revoke")}
                      </Button>
                    </Show>
                  </li>
                )}
              </For>
            </ul>
          )}
        </Show>
      </Show>
      <Show when={revoking()}>
        {(device) => (
          <ConfirmDialog
            title={t("settings.revokeTitle")}
            message={t("settings.revokeBody", { label: device().deviceLabel })}
            confirmLabel={t("settings.revoke")}
            danger
            onCancel={() => setRevoking(null)}
            onConfirm={() => void revoke()}
          />
        )}
      </Show>
    </div>
  );
}

/** Account area of Settings: sync setup for a local workspace, or the signed-in account. */
export function AccountSection(): JSX.Element {
  const app = useApp();
  const status = () => app.session.status();
  const [joining, setJoining] = createSignal(false);
  const [confirmSignOut, setConfirmSignOut] = createSignal(false);

  const signOut = async () => {
    setConfirmSignOut(false);
    await app.guard(() => app.session.signOut());
  };

  return (
    <section class="settings-section" aria-labelledby="account-title">
      <h2 id="account-title">{t("settings.account")}</h2>
      <Switch>
        <Match when={status()?.state === "signed_in"}>
          <dl class="facts">
            <dt>{t("auth.username")}</dt>
            <dd>{status()?.username}</dd>
            <dt>{t("auth.serverUrl")}</dt>
            <dd>{status()?.serverUrl}</dd>
          </dl>
          <ChangePasswordForm />
          <DeviceList />
          <Button variant="danger" onClick={() => setConfirmSignOut(true)}>
            {t("settings.signOut")}
          </Button>
          <Show when={confirmSignOut()}>
            <ConfirmDialog
              title={t("settings.signOutTitle")}
              message={t("settings.signOutBody")}
              confirmLabel={t("settings.signOut")}
              danger
              onCancel={() => setConfirmSignOut(false)}
              onConfirm={() => void signOut()}
            />
          </Show>
        </Match>
        <Match when={status()?.state === "local"}>
          <p>{t("settings.localOnly")}</p>
          <Button variant="primary" icon="sync" onClick={() => setJoining(true)}>
            {t("settings.enableSync")}
          </Button>
          <Show when={joining()}>
            <Dialog title={t("settings.enableSync")} onClose={() => setJoining(false)} dismissOnBackdrop={false} wide>
              <AccountFlow />
            </Dialog>
          </Show>
        </Match>
      </Switch>
    </section>
  );
}
