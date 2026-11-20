import { For, Show, createSignal, onMount, type JSX } from "solid-js";
import type { ReminderEntry } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { formatDateTime, formatRelative } from "../../lib/format.ts";
import { useApp } from "../../state/app.tsx";
import { Button, IconButton } from "../common/button.tsx";
import { EmptyState, ErrorBanner } from "../common/states.tsx";
import { Menu, type MenuEntry } from "../common/menu.tsx";
import { tomorrowMorning } from "./reminder-dialog.tsx";

const MINUTE = 60_000;

/** Upcoming reminders with Done and Snooze; completed ones can be shown too. */
export function RemindersPanel(): JSX.Element {
  const app = useApp();
  const { reminders } = app;
  const [snoozing, setSnoozing] = createSignal<{ id: string; at: { x: number; y: number } } | null>(null);
  const [showDone, setShowDone] = createSignal(false);
  onMount(() => void reminders.refresh());

  const snoozeEntries = (id: string): MenuEntry[] => {
    const until = (ms: number) => () => void app.guard(() => reminders.snooze(id, Date.now() + ms));
    return [
      { kind: "item", label: t("reminder.snooze10"), onSelect: until(10 * MINUTE) },
      { kind: "item", label: t("reminder.snooze60"), onSelect: until(60 * MINUTE) },
      { kind: "item", label: t("reminder.snoozeTomorrow"), onSelect: () => void app.guard(() => reminders.snooze(id, tomorrowMorning())) },
    ];
  };

  const row = (entry: ReminderEntry, done: boolean): JSX.Element => {
    const overdue = () => !done && entry.dueAt <= Date.now();
    return (
      <li class="reminder" data-overdue={overdue() ? "true" : undefined} data-gone={entry.pageVisible ? undefined : "true"}>
        <div class="reminder-main">
          <Show when={entry.pageVisible} fallback={<span class="reminder-title">{entry.pageTitle || t("tree.untitled")}</span>}>
            <button type="button" class="link-button reminder-title" onClick={() => app.openPage(entry.reminder.pageId)}>{entry.pageTitle || t("tree.untitled")}</button>
          </Show>
          <span class="reminder-when" title={formatDateTime(entry.dueAt)}>{overdue() ? t("reminder.overdue", { when: formatRelative(entry.dueAt) }) : formatRelative(entry.dueAt)}</span>
          <Show when={entry.reminder.note}><span class="reminder-note">{entry.reminder.note}</span></Show>
          <Show when={!entry.pageVisible}><span class="field-hint">{t("reminder.pageGone")}</span></Show>
        </div>
        <div class="reminder-actions">
          <Button onClick={() => void app.guard(() => reminders.setDone(entry.reminder.id, !done))}>{done ? t("reminder.reopen") : t("common.done")}</Button>
          <Show when={!done && entry.pageVisible}>
            <Button
              aria-haspopup="menu"
              onClick={(event) => {
                const rect = event.currentTarget.getBoundingClientRect();
                setSnoozing({ id: entry.reminder.id, at: { x: rect.left, y: rect.bottom + 4 } });
              }}
            >
              {t("reminder.snooze")}
            </Button>
          </Show>
          <IconButton icon="trash" size={14} label={t("reminder.delete")} onClick={() => void app.guard(() => reminders.remove(entry.reminder.id))} />
        </div>
      </li>
    );
  };

  return (
    <aside class="side-panel" aria-label={t("reminder.panelTitle")}>
      <header class="side-panel-head">
        <h2>{t("reminder.panelTitle")}</h2>
        <IconButton icon="close" size={14} label={t("common.close")} onClick={() => app.ui.closeSidePanel()} />
      </header>
      <ErrorBanner error={reminders.error()} />
      <Show when={reminders.entries().length > 0} fallback={<EmptyState icon="bell" title={t("reminder.emptyTitle")}>{t("reminder.emptyBody")}</EmptyState>}>
        <ul class="reminder-list" aria-label={t("reminder.upcoming")}>
          <For each={reminders.entries()}>{(entry) => row(entry, false)}</For>
        </ul>
      </Show>
      <label class="checkbox">
        <input
          type="checkbox"
          checked={showDone()}
          onChange={(event) => {
            setShowDone(event.currentTarget.checked);
            void reminders.refresh({ includeDone: event.currentTarget.checked });
          }}
        />
        <span>{t("reminder.showDone")}</span>
      </label>
      <Show when={showDone() && reminders.done().length > 0}>
        <ul class="reminder-list" aria-label={t("reminder.completed")}>
          <For each={reminders.done()}>{(entry) => row(entry, true)}</For>
        </ul>
      </Show>
      <Show when={snoozing()}>{(open) => <Menu label={t("reminder.snooze")} at={open().at} entries={snoozeEntries(open().id)} onClose={() => setSnoozing(null)} />}</Show>
    </aside>
  );
}
