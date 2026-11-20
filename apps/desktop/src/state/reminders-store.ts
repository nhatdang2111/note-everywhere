import { createMemo, createSignal } from "solid-js";
import type { Core } from "../api/index.ts";
import type { ApiError, Reminder, ReminderAddArgs, ReminderEntry } from "../api/contract.ts";
import { toApiError } from "../api/errors.ts";

export interface RemindersStore {
  /** Reminders that are not done, soonest first. */
  entries: () => ReminderEntry[];
  /** Reminders that are done, newest first; empty until `refresh({ includeDone: true })`. */
  done: () => ReminderEntry[];
  loaded: () => boolean;
  error: () => ApiError | null;
  /** How many open reminders are due now or overdue. */
  dueCount: () => number;
  refresh(options?: { includeDone?: boolean }): Promise<void>;
  add(args: Omit<ReminderAddArgs, "utcOffsetMinutes">): Promise<Reminder>;
  setDone(id: string, done: boolean): Promise<void>;
  snooze(id: string, until: number): Promise<void>;
  reschedule(id: string, at: number): Promise<void>;
  setNote(id: string, note: string): Promise<void>;
  remove(id: string): Promise<void>;
}

/** The person's current UTC offset in minutes, as reminders store it. */
export const utcOffsetMinutes = (): number => -new Date().getTimezoneOffset();

export function createRemindersStore(core: Core, now: () => number = Date.now): RemindersStore {
  const [all, setAll] = createSignal<ReminderEntry[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [error, setError] = createSignal<ApiError | null>(null);
  let includeDone = false;

  const entries = createMemo(() => all().filter((entry) => !entry.reminder.done));
  const done = createMemo(() => all().filter((entry) => entry.reminder.done).sort((a, b) => b.dueAt - a.dueAt));

  async function refresh(options?: { includeDone?: boolean }): Promise<void> {
    if (options?.includeDone !== undefined) includeDone = options.includeDone;
    try {
      setAll(await core.api.reminderList({ includeDone }));
      setError(null);
    } catch (failure) {
      setError(toApiError(failure));
    } finally {
      setLoaded(true);
    }
  }

  // Changes made here or noticed from another device both arrive as this event.
  void core.events.on("reminders:changed", () => void refresh());

  const mutate = async <T,>(run: () => Promise<T>): Promise<T> => {
    const result = await run();
    await refresh();
    return result;
  };

  return {
    entries,
    done,
    loaded,
    error,
    dueCount: createMemo(() => entries().filter((entry) => entry.pageVisible && entry.dueAt <= now()).length),
    refresh,
    add: (args) => mutate(() => core.api.reminderAdd({ ...args, utcOffsetMinutes: utcOffsetMinutes() })),
    setDone: (id, value) => mutate(() => core.api.reminderSetDone({ id, done: value })),
    snooze: (id, until) => mutate(() => core.api.reminderSnooze({ id, until })),
    reschedule: (id, at) => mutate(() => core.api.reminderReschedule({ id, at })),
    setNote: (id, note) => mutate(() => core.api.reminderSetNote({ id, note })),
    remove: (id) => mutate(() => core.api.reminderDelete({ id })),
  };
}
