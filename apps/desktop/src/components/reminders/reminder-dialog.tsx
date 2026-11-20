import { For, Show, createResource, createSignal, type JSX } from "solid-js";
import type { NodeId } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { requestNotificationPermission } from "../../lib/notify.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Dialog } from "../common/dialog.tsx";
import { TextField } from "../common/form.tsx";

const pad = (n: number) => String(n).padStart(2, "0");
/** `YYYY-MM-DDTHH:MM` in the person's own time zone, as a datetime-local input wants it. */
export const localInput = (ms: number): string => {
  const date = new Date(ms);
  return `${date.getFullYear()}-${pad(date.getMonth() + 1)}-${pad(date.getDate())}T${pad(date.getHours())}:${pad(date.getMinutes())}`;
};

export function tomorrowMorning(now = Date.now()): number {
  const date = new Date(now);
  date.setDate(date.getDate() + 1);
  date.setHours(9, 0, 0, 0);
  return date.getTime();
}

export interface ReminderDialogProps {
  pageId: NodeId;
  databaseId?: NodeId;
  /** Start on "follow this date property". */
  propertyId?: string;
  onClose(): void;
}

/** Remind about a page at a time, or about a row on the date of one of its date properties. */
export function ReminderDialog(props: ReminderDialogProps): JSX.Element {
  const app = useApp();
  const [schema] = createResource(() => props.databaseId, (id) => app.core.api.dbSchema({ id }));
  const dateProperties = () => (schema()?.properties ?? []).filter((prop) => prop.type === "date");
  const [mode, setMode] = createSignal<string>(props.propertyId ?? "time");
  const [when, setWhen] = createSignal(localInput(tomorrowMorning()));
  const [note, setNote] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  const valid = () => mode() !== "time" || (when() !== "" && !Number.isNaN(new Date(when()).getTime()));

  const submit = async () => {
    if (!valid()) return;
    setBusy(true);
    await requestNotificationPermission();
    const ok = await app.guard(() =>
      app.reminders.add(mode() === "time" ? { pageId: props.pageId, at: new Date(when()).getTime(), note: note() } : { pageId: props.pageId, propertyId: mode(), note: note() }),
    );
    setBusy(false);
    if (ok) {
      app.toasts.push("success", t("reminder.set"));
      props.onClose();
    }
  };

  return (
    <Dialog
      title={t("reminder.dialogTitle", { title: app.pageTitle(props.pageId) || t("tree.untitled") })}
      onClose={props.onClose}
      dismissOnBackdrop={false}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" busy={busy()} disabled={!valid()} onClick={() => void submit()}>{t("reminder.setButton")}</Button>
        </>
      }
    >
      <form onSubmit={(event) => (event.preventDefault(), void submit())}>
        <fieldset class="reminder-mode">
          <legend>{t("reminder.when")}</legend>
          <label class="checkbox">
            <input type="radio" name="reminder-mode" checked={mode() === "time"} onChange={() => setMode("time")} />
            <span>{t("reminder.atTime")}</span>
          </label>
          <For each={dateProperties()}>
            {(prop) => (
              <label class="checkbox">
                <input type="radio" name="reminder-mode" checked={mode() === prop.id} onChange={() => setMode(prop.id)} />
                <span>{t("reminder.onProperty", { name: prop.name })}</span>
              </label>
            )}
          </For>
        </fieldset>
        <Show when={mode() === "time"}>
          <div class="field">
            <label for="reminder-when">{t("reminder.time")}</label>
            <input id="reminder-when" type="datetime-local" data-autofocus value={when()} onInput={(event) => setWhen(event.currentTarget.value)} />
          </div>
        </Show>
        <Show when={mode() !== "time"}>
          <p class="field-hint">{t("reminder.propertyHint")}</p>
        </Show>
        <TextField label={t("reminder.note")} value={note()} onInput={setNote} maxLength={200} />
      </form>
    </Dialog>
  );
}
