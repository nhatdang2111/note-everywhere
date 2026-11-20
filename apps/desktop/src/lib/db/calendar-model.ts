import type { DateValue, QueryRow } from "../../api/contract.ts";

/** Month grid and date arithmetic for the calendar view. Days are `YYYY-MM-DD`; all arithmetic is in UTC so it never skips a day. */

export interface CalendarDay {
  date: string;
  inMonth: boolean;
}

export interface MonthGrid {
  year: number;
  /** 0-11. */
  month: number;
  days: CalendarDay[];
  from: string;
  to: string;
}

const pad = (n: number) => String(n).padStart(2, "0");
export const dayText = (date: Date): string => `${date.getUTCFullYear()}-${pad(date.getUTCMonth() + 1)}-${pad(date.getUTCDate())}`;
const parseDay = (text: string): Date => new Date(`${text.slice(0, 10)}T00:00:00Z`);

export const addDays = (text: string, days: number): string => {
  const date = parseDay(text);
  date.setUTCDate(date.getUTCDate() + days);
  return dayText(date);
};

export const daysBetween = (from: string, to: string): number => Math.round((parseDay(to).getTime() - parseDay(from).getTime()) / 86_400_000);

/** Six full weeks around the month, starting on Monday unless `weekStartsOn` (0 = Sunday) says otherwise. */
export function monthGrid(year: number, month: number, weekStartsOn = 1): MonthGrid {
  const first = new Date(Date.UTC(year, month, 1));
  const lead = (first.getUTCDay() - weekStartsOn + 7) % 7;
  const start = dayText(new Date(Date.UTC(year, month, 1 - lead)));
  const days: CalendarDay[] = Array.from({ length: 42 }, (_, at) => {
    const date = addDays(start, at);
    return { date, inMonth: parseDay(date).getUTCMonth() === month };
  });
  return { year, month, days, from: days[0]?.date ?? start, to: days[41]?.date ?? start };
}

export function shiftMonth(year: number, month: number, delta: number): { year: number; month: number } {
  const date = new Date(Date.UTC(year, month + delta, 1));
  return { year: date.getUTCFullYear(), month: date.getUTCMonth() };
}

/** The date a row has in `propertyId`, if it has one. */
export function dateOf(row: QueryRow, propertyId: string): DateValue | undefined {
  const value = row.values[propertyId];
  return value?.type === "date" ? value.value : undefined;
}

/** Rows by day, a row with a date range appearing on every day it covers inside the grid. */
export function placeRows(grid: MonthGrid, rows: QueryRow[], propertyId: string): Map<string, QueryRow[]> {
  const byDay = new Map<string, QueryRow[]>(grid.days.map((day) => [day.date, []]));
  for (const row of rows) {
    const value = dateOf(row, propertyId);
    if (!value) continue;
    const start = value.start.slice(0, 10);
    const end = (value.end ?? value.start).slice(0, 10);
    for (let day = start < grid.from ? grid.from : start; day <= end && day <= grid.to; day = addDays(day, 1)) byDay.get(day)?.push(row);
  }
  return byDay;
}

/** The date after dragging to `day`: the start lands there, the length and the times stay. */
export function moveDate(value: DateValue, day: string): DateValue {
  const shift = daysBetween(value.start.slice(0, 10), day);
  const move = (text: string) => `${addDays(text, shift)}${text.length > 10 ? text.slice(10) : ""}`;
  return value.end === undefined ? { start: move(value.start) } : { start: move(value.start), end: move(value.end) };
}

/** The new value for dropping `rowId` on `day`; null when the row has no date or is already there. */
export function planDayDrop(rows: QueryRow[], rowId: string, propertyId: string, day: string): DateValue | null {
  const row = rows.find((candidate) => candidate.id === rowId);
  const value = row && dateOf(row, propertyId);
  if (!value || value.start.slice(0, 10) === day) return null;
  return moveDate(value, day);
}
