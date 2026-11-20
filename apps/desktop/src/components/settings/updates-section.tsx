import { Show, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { formatBytes, formatDateTime } from "../../lib/format.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";

/** Current version, what the update server said, and the three explicit steps: check, download, install. */
export function UpdatesSection(): JSX.Element {
  const { updates } = useApp();
  const [confirming, setConfirming] = createSignal(false);
  const status = () => updates.status();
  const state = () => status()?.state ?? "idle";
  const busy = () => state() === "checking" || state() === "downloading";
  const percent = () => {
    const progress = updates.progress();
    return progress?.totalBytes ? Math.min(100, Math.round((progress.receivedBytes / progress.totalBytes) * 100)) : null;
  };

  return (
    <section class="settings-section" aria-labelledby="updates-title">
      <h2 id="updates-title">{t("updates.title")}</h2>
      <p class="view-intro">{t("updates.current", { version: status()?.currentVersion ?? "" })}</p>
      <p role="status" class="update-state" data-state={state()}>
        {t(`updates.state.${state()}`)}
        <Show when={status()?.version && ["available", "downloading", "ready"].includes(state())}>
          {" "}
          {t("updates.version", { version: status()?.version ?? "" })}
        </Show>
      </p>
      <Show when={status()?.message}>
        <p class="field-hint">{status()?.message}</p>
      </Show>
      <Show when={status()?.lastCheckedAt}>
        <p class="field-hint">{t("updates.lastChecked", { when: formatDateTime(status()?.lastCheckedAt ?? 0) })}</p>
      </Show>
      <Show when={status()?.notes && ["available", "downloading", "ready"].includes(state())}>
        <details class="update-notes">
          <summary>{t("updates.notes")}</summary>
          <pre>{status()?.notes}</pre>
        </details>
      </Show>
      <Show when={state() === "downloading"}>
        <div class="progress" role="progressbar" aria-label={t("updates.downloading")} aria-valuemin={0} aria-valuemax={100} aria-valuenow={percent() ?? undefined}>
          <div class="progress-bar" style={{ width: `${percent() ?? 100}%` }} />
        </div>
        <Show when={updates.progress()}>
          {(progress) => <p class="field-hint">{formatBytes(progress().receivedBytes)}{progress().totalBytes ? ` / ${formatBytes(progress().totalBytes as number)}` : ""}</p>}
        </Show>
      </Show>
      <Show when={state() !== "unavailable"}>
        <div class="header-actions">
          <Button icon="sync" busy={state() === "checking"} disabled={busy() || state() === "ready"} onClick={() => void updates.check()}>
            {t("updates.check")}
          </Button>
          <Show when={state() === "available"}>
            <Button variant="primary" icon="download" onClick={() => void updates.download()}>
              {t("updates.download")}
            </Button>
          </Show>
          <Show when={state() === "ready"}>
            <Button variant="primary" onClick={() => setConfirming(true)}>
              {t("updates.install")}
            </Button>
          </Show>
        </div>
      </Show>
      <Show when={confirming()}>
        <ConfirmDialog
          title={t("updates.confirmTitle")}
          message={t("updates.confirmBody", { version: status()?.version ?? "" })}
          confirmLabel={t("updates.install")}
          onCancel={() => setConfirming(false)}
          onConfirm={() => {
            setConfirming(false);
            void updates.install();
          }}
        />
      </Show>
    </section>
  );
}
