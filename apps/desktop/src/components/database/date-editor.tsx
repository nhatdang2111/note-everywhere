import { Show, type JSX } from "solid-js";
import type { CellValue, DateValue } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { Button } from "../common/button.tsx";
import { Popover } from "../common/popover.tsx";

export interface DateEditorProps {
  anchor: HTMLElement;
  value: CellValue | undefined;
  onChange(value: CellValue | null): void;
  onClose(): void;
}

const dayOf = (text: string) => text.slice(0, 10);
const timeOf = (text: string) => (text.length > 10 ? text.slice(11, 16) : "");
const today = () => new Date().toLocaleDateString("sv-SE");

/** Start date, optional end date, and an optional time on each: every change is saved at once. */
export function DateEditor(props: DateEditorProps): JSX.Element {
  const current = (): DateValue | undefined => (props.value?.type === "date" ? props.value.value : undefined);
  const hasTime = () => (current()?.start.length ?? 0) > 10;
  const emit = (next: DateValue) => props.onChange({ type: "date", value: next });
  const join = (day: string, time: string) => (time ? `${day}T${time}` : day);

  const setStart = (day: string) => {
    if (!day) return;
    const value = current();
    emit({ start: join(day, timeOf(value?.start ?? "")), ...(value?.end ? { end: join(day > dayOf(value.end) ? day : dayOf(value.end), timeOf(value.end)) } : {}) });
  };
  const setStartTime = (time: string) => {
    const value = current();
    if (value) emit({ ...value, start: join(dayOf(value.start), time) });
  };
  const toggleTime = (on: boolean) => {
    const value = current() ?? { start: today() };
    emit({
      start: join(dayOf(value.start), on ? "09:00" : ""),
      ...(value.end ? { end: join(dayOf(value.end), on ? "10:00" : "") } : {}),
    });
  };
  const toggleEnd = (on: boolean) => {
    const value = current() ?? { start: today() };
    emit(on ? { ...value, end: value.start } : { start: value.start });
  };
  const setEnd = (day: string) => {
    const value = current();
    if (!value || !day) return;
    emit({ ...value, end: join(day < dayOf(value.start) ? dayOf(value.start) : day, timeOf(value.end ?? "")) });
  };
  const setEndTime = (time: string) => {
    const value = current();
    if (value?.end) emit({ ...value, end: join(dayOf(value.end), time) });
  };

  return (
    <Popover anchor={props.anchor} label={t("db.editDate")} onClose={props.onClose} class="popover-date">
      <div class="field">
        <label for="date-start">{t("db.dateStart")}</label>
        <div class="date-inputs">
          <input id="date-start" type="date" data-autofocus value={dayOf(current()?.start ?? "")} onChange={(event) => setStart(event.currentTarget.value)} />
          <Show when={hasTime()}>
            <input type="time" aria-label={t("db.dateStartTime")} value={timeOf(current()?.start ?? "")} onChange={(event) => setStartTime(event.currentTarget.value)} />
          </Show>
        </div>
      </div>
      <label class="checkbox">
        <input type="checkbox" checked={Boolean(current()?.end)} disabled={!current()} onChange={(event) => toggleEnd(event.currentTarget.checked)} />
        <span>{t("db.dateEnd")}</span>
      </label>
      <Show when={current()?.end}>
        {(end) => (
          <div class="date-inputs">
            <input type="date" aria-label={t("db.dateEndDay")} value={dayOf(end())} onChange={(event) => setEnd(event.currentTarget.value)} />
            <Show when={hasTime()}>
              <input type="time" aria-label={t("db.dateEndTime")} value={timeOf(end())} onChange={(event) => setEndTime(event.currentTarget.value)} />
            </Show>
          </div>
        )}
      </Show>
      <label class="checkbox">
        <input type="checkbox" checked={hasTime()} disabled={!current()} onChange={(event) => toggleTime(event.currentTarget.checked)} />
        <span>{t("db.dateTime")}</span>
      </label>
      <div class="popover-footer">
        <Button variant="ghost" onClick={() => (props.onChange(null), props.onClose())}>
          {t("db.clearCell")}
        </Button>
        <Button variant="primary" onClick={props.onClose}>
          {t("common.done")}
        </Button>
      </div>
    </Popover>
  );
}
