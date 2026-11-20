import { createSignal } from "solid-js";
import type { NodeId } from "../api/contract.ts";
import { readJson, writeJson } from "../lib/storage.ts";

const ROWS_KEY = "ne.rows";
const LIMIT = 400;

interface RowInfo {
  databaseId: NodeId;
  title: string;
}

type Known = Record<NodeId, RowInfo>;

const isKnown = (value: unknown): value is Known =>
  typeof value === "object" &&
  value !== null &&
  Object.values(value).every((entry) => typeof (entry as RowInfo)?.databaseId === "string" && typeof (entry as RowInfo)?.title === "string");

/**
 * Rows are pages that `treeSnapshot` does not list, so the tree cannot say which
 * database a row belongs to or what it is called. The database views tell us
 * when they show a row; this remembers it (the most recent few hundred, in
 * local storage) so an open row keeps its title, tab and place after a reload.
 */
export interface RowRegistry {
  databaseOf(rowId: NodeId): NodeId | undefined;
  title(rowId: NodeId): string | undefined;
  remember(rowId: NodeId, databaseId: NodeId, title: string): void;
  /** Update the title of a row that is already known. */
  retitle(rowId: NodeId, title: string): void;
}

export function createRowRegistry(): RowRegistry {
  const [known, setKnown] = createSignal<Known>(readJson(ROWS_KEY, {}, isKnown));
  const commit = (next: Known) => {
    const keys = Object.keys(next);
    const trimmed = keys.length > LIMIT ? Object.fromEntries(keys.slice(keys.length - LIMIT).map((key) => [key, next[key] as RowInfo])) : next;
    setKnown(trimmed);
    writeJson(ROWS_KEY, trimmed);
  };
  return {
    databaseOf: (rowId) => known()[rowId]?.databaseId,
    title: (rowId) => known()[rowId]?.title,
    remember(rowId, databaseId, title) {
      const current = known()[rowId];
      if (current?.databaseId === databaseId && current.title === title) return;
      const { [rowId]: _old, ...rest } = known();
      commit({ ...rest, [rowId]: { databaseId, title } });
    },
    retitle(rowId, title) {
      const current = known()[rowId];
      if (current && current.title !== title) commit({ ...known(), [rowId]: { ...current, title } });
    },
  };
}
