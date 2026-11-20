import { For, Show, createSignal, type JSX } from "solid-js";
import type { QueryRow } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { dayText, placeRows, shiftMonth } from "../../lib/db/calendar-model.ts";
import { locale } from "../../i18n/index.ts";
import { Button, IconButton } from "../common/button.tsx";
import { EmptyState } from "../common/states.tsx";
import { useDatabase, useDatabaseActions } from "./database-context.tsx";

/** A month grid placing rows by a date property; drag a row to another day to change its date. */
export function CalendarView(): JSX.Element {
  const db = useDatabase();
  const actions = useDatabaseActions();
  const [dragging, setDragging] = createSignal<string | null>(null);
  const [adding, setAdding] = createSignal<string | null>(null);
  const property = () => {
    const id = db.view()?.dateProperty;
    return db.schema()?.properties.find((prop) => prop.id === id && prop.type === "date");
  };
  const dates = () => (db.schema()?.properties ?? []).filter((prop) => prop.type === "date");
  const placed = () => placeRows(db.calendar().grid, db.calendar().rows, property()?.id ?? "");
  const monthTitle = () => new Intl.DateTimeFormat(locale(), { month: "long", year: "numeric" }).format(new Date(db.calendar().grid.year, db.calendar().grid.month, 1));
  const weekdays = () => Array.from({ length: 7 }, (_, at) => new Intl.DateTimeFormat(locale(), { weekday: "short" }).format(new Date(Date.UTC(2024, 0, 1 + at))));
  const today = dayText(new Date());

  const go = (delta: number) => {
    const { year, month } = shiftMonth(db.calendar().grid.year, db.calendar().grid.month, delta);
    db.setMonth(year, month);
  };

  const create = async (day: string, title: string) => {
    const prop = property();
    setAdding(null);
    if (!prop || !title.trim()) return;
    await db.createRow({ title: title.trim(), values: { [prop.id]: { type: "date", value: { start: day } } } });
  };

  const chip = (row: QueryRow): JSX.Element => (
    <button
      type="button"
      class="cal-row"
      data-row={row.id}
      draggable="true"
      onClick={() => actions.openRow(row.id, row.title)}
      onDragStart={(event) => (event.dataTransfer?.setData("application/x-ne-row", row.id), setDragging(row.id))}
      onDragEnd={() => setDragging(null)}
    >
      {row.title || t("tree.untitled")}
    </button>
  );

  return (
    <Show
      when={property()}
      fallback={
        <EmptyState icon="callout" title={t("db.calendarNeedsDate")} action={
          <Show when={dates().length === 0} fallback={<span />}>
            <Button variant="primary" onClick={() => actions.openSchemaDialog({ kind: "add-property" })}>{t("db.addProperty")}</Button>
          </Show>
        }>
          {dates().length > 0 ? t("db.calendarPickDate") : t("db.calendarNoDate")}
        </EmptyState>
      }
    >
      <div class="calendar">
        <div class="calendar-head">
          <IconButton icon="arrow-left" label={t("db.previousMonth")} onClick={() => go(-1)} />
          <h3 aria-live="polite">{monthTitle()}</h3>
          <IconButton icon="arrow-right" label={t("db.nextMonth")} onClick={() => go(1)} />
          <Button variant="ghost" onClick={() => db.setMonth(new Date().getFullYear(), new Date().getMonth())}>{t("db.today")}</Button>
        </div>
        <div class="calendar-grid" role="grid" aria-label={monthTitle()}>
          <div class="calendar-weekdays" role="row">
            <For each={weekdays()}>{(name) => <div role="columnheader">{name}</div>}</For>
          </div>
          <For each={Array.from({ length: 6 }, (_, week) => week)}>
            {(week) => (
              <div class="calendar-week" role="row">
                <For each={db.calendar().grid.days.slice(week * 7, week * 7 + 7)}>
                  {(day) => (
                    <div
                      class="calendar-day"
                      role="gridcell"
                      data-day={day.date}
                      data-outside={day.inMonth ? undefined : "true"}
                      data-today={day.date === today ? "true" : undefined}
                      onDragOver={(event) => dragging() && event.preventDefault()}
                      onDrop={(event) => {
                        event.preventDefault();
                        const id = dragging();
                        setDragging(null);
                        if (id) void db.moveRowToDay(id, day.date);
                      }}
                    >
                      <div class="calendar-day-head">
                        <span class="calendar-date">{Number(day.date.slice(8))}</span>
                        <IconButton icon="plus" size={12} label={t("db.addOnDay", { day: day.date })} onClick={() => setAdding(day.date)} />
                      </div>
                      <For each={(placed().get(day.date) ?? []).map((row) => row.id)}>{(rowId) => <Show when={(placed().get(day.date) ?? []).find((row) => row.id === rowId)}>{(row) => chip(row())}</Show>}</For>
                      <Show when={adding() === day.date}>
                        <input
                          class="board-add-input"
                          aria-label={t("db.newRowOnDay", { day: day.date })}
                          placeholder={t("db.newCardPlaceholder")}
                          ref={(element) => queueMicrotask(() => element.focus())}
                          onKeyDown={(event) => {
                            if (event.key === "Enter") (event.preventDefault(), void create(day.date, event.currentTarget.value));
                            else if (event.key === "Escape") (event.stopPropagation(), setAdding(null));
                          }}
                          onBlur={() => setAdding(null)}
                        />
                      </Show>
                    </div>
                  )}
                </For>
              </div>
            )}
          </For>
        </div>
      </div>
    </Show>
  );
}
