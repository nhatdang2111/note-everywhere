import type { CoreApi, NodeId, Reminder, ReminderEntry } from "./contract.ts";
import { apiError } from "./errors.ts";
import { randomId, type Bus } from "./memory-bus.ts";
import type { DbStore } from "./memory-db.ts";
import type { TreeStore } from "./memory-tree.ts";

type ReminderMethods = Pick<
  CoreApi,
  "reminderAdd" | "reminderList" | "reminderSetDone" | "reminderSnooze" | "reminderReschedule" | "reminderSetNote" | "reminderDelete"
>;

export interface Reminders extends ReminderMethods {
  /** Fire whatever is due now. Normally a timer does this; tests call it after moving the clock. */
  check(): void;
}

/** A reminder never fires for a moment older than this when the device looks. */
const STALE_MS = 10 * 60 * 1000;
const DEFAULT_HOUR = 9;
const MAX_TIMER_MS = 2 ** 31 - 1;

/** Wall-clock `YYYY-MM-DD[THH:MM]` in the person's UTC offset to ms since the epoch; a date alone is 09:00. */
function momentOf(text: string, offsetMinutes: number): number {
  const [datePart = "", timePart] = text.split("T");
  const [year = 0, month = 1, day = 1] = datePart.split("-").map(Number);
  const [hour, minute] = timePart ? timePart.split(":").map(Number) : [DEFAULT_HOUR, 0];
  return Date.UTC(year, month - 1, day, hour ?? DEFAULT_HOUR, minute ?? 0) - offsetMinutes * 60_000;
}

export function createReminders(tree: TreeStore, db: DbStore, bus: Bus, now: () => number, nativeShown: boolean): Reminders {
  const all = new Map<string, Reminder>();
  const fired = new Set<string>();
  let timer: ReturnType<typeof setTimeout> | undefined;

  const need = (id: string): Reminder => {
    const reminder = all.get(id);
    if (!reminder) throw apiError("not_found", "That reminder does not exist.");
    return reminder;
  };

  const rowDatabase = (pageId: NodeId): NodeId | undefined => {
    const parent = tree.get(pageId)?.parentId;
    return parent && tree.get(parent)?.kind === "database" ? parent : undefined;
  };

  function dueAt(reminder: Reminder): number {
    if (reminder.snoozedUntil !== undefined) return reminder.snoozedUntil;
    if (reminder.propertyId) {
      const databaseId = rowDatabase(reminder.pageId);
      const cell = databaseId ? db.rowCells(databaseId, reminder.pageId)[reminder.propertyId] : undefined;
      if (cell?.type === "date") return momentOf(cell.value.start, reminder.utcOffsetMinutes);
    }
    return reminder.at;
  }

  const entryOf = (reminder: Reminder): ReminderEntry => ({
    reminder: { ...reminder },
    dueAt: dueAt(reminder),
    pageTitle: tree.get(reminder.pageId)?.title ?? "",
    pageVisible: tree.isLive(reminder.pageId),
  });

  const upcoming = () => [...all.values()].filter((reminder) => !reminder.done && tree.isLive(reminder.pageId));

  function schedule(): void {
    clearTimeout(timer);
    const current = now();
    const next = upcoming()
      .map((reminder) => ({ reminder, due: dueAt(reminder) }))
      .filter(({ reminder, due }) => !fired.has(`${reminder.id}@${due}`) && due > current - STALE_MS)
      .sort((a, b) => a.due - b.due)[0];
    if (next) timer = setTimeout(check, Math.min(Math.max(next.due - current, 0), MAX_TIMER_MS));
  }

  function check(): void {
    const current = now();
    const due: ReminderEntry[] = [];
    for (const reminder of upcoming()) {
      const at = dueAt(reminder);
      const key = `${reminder.id}@${at}`;
      if (at > current || fired.has(key)) continue;
      fired.add(key);
      if (current - at <= STALE_MS) due.push(entryOf(reminder));
    }
    if (due.length > 0) bus.emit("reminder:due", { reminders: due, nativeShown });
    schedule();
  }

  const changed = () => {
    bus.emit("reminders:changed", {});
    schedule();
  };

  return {
    check,

    async reminderAdd({ pageId, propertyId, at, utcOffsetMinutes, note }) {
      const page = tree.get(pageId);
      if (!page || page.kind !== "page" || !tree.isLive(pageId)) throw apiError("not_found", "That page is not available.");
      if (propertyId) {
        const databaseId = rowDatabase(pageId);
        const prop = databaseId ? db.schemaOf(databaseId).properties.find((candidate) => candidate.id === propertyId) : undefined;
        if (!prop || prop.type !== "date") throw apiError("invalid_input", "A property reminder needs a date property of the row.");
      } else if (at === undefined || !Number.isFinite(at)) {
        throw apiError("invalid_input", "Choose when to be reminded.");
      }
      const reminder: Reminder = {
        id: randomId("rem"),
        pageId,
        ...(propertyId ? { propertyId } : {}),
        at: propertyId ? (at ?? now()) : (at as number),
        utcOffsetMinutes,
        note: note?.trim() ?? "",
        done: false,
        createdAt: now(),
      };
      all.set(reminder.id, reminder);
      changed();
      queueMicrotask(check);
      return { ...reminder };
    },
    async reminderList({ includeDone }) {
      return [...all.values()]
        .filter((reminder) => includeDone || !reminder.done)
        .map(entryOf)
        .sort((a, b) => a.dueAt - b.dueAt);
    },
    async reminderSetDone({ id, done }) {
      need(id).done = done;
      changed();
    },
    async reminderSnooze({ id, until }) {
      const reminder = need(id);
      reminder.snoozedUntil = until;
      reminder.done = false;
      changed();
    },
    async reminderReschedule({ id, at }) {
      const reminder = need(id);
      reminder.at = at;
      delete reminder.snoozedUntil;
      reminder.done = false;
      changed();
    },
    async reminderSetNote({ id, note }) {
      need(id).note = note.trim();
      changed();
    },
    async reminderDelete({ id }) {
      need(id);
      all.delete(id);
      changed();
    },
  };
}
