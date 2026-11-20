import type { QueryBucket, QueryRow } from "../../api/contract.ts";

/** Pure planning of card moves on a board, so dragging, menus and keys all go through one place. */

export interface CardMove {
  rowId: string;
  /** Column the card leaves. */
  from: string | null;
  /** Column the card lands in: an option id, "true"/"false" for a checkbox, null for no value. */
  group: string | null;
  /** The card it goes in front of, or null for the end of the column. */
  beforeId: string | null;
}

export type BoardColumn = QueryBucket;

const column = (columns: BoardColumn[], key: string | null) => columns.find((candidate) => candidate.key === key);

/** The move for dropping `rowId` into column `group` before `beforeId`; null when it would change nothing. */
export function planCardMove(columns: BoardColumn[], rowId: string, from: string | null, group: string | null, beforeId: string | null): CardMove | null {
  const source = column(columns, from);
  const target = column(columns, group);
  if (!source || !target || !source.rows.some((row) => row.id === rowId)) return null;
  if (beforeId === rowId) return null;
  if (from === group) {
    const ids = source.rows.map((row) => row.id);
    const at = ids.indexOf(rowId);
    const next = beforeId === null ? ids.length - 1 : ids.indexOf(beforeId) - (ids.indexOf(beforeId) > at ? 1 : 0);
    if (beforeId !== null && ids.indexOf(beforeId) < 0) return null;
    if (next === at) return null;
  }
  return { rowId, from, group, beforeId };
}

/** The columns as they look right after a move, for showing it before the core confirms. */
export function applyCardMove(columns: BoardColumn[], move: CardMove): BoardColumn[] {
  const card = column(columns, move.from)?.rows.find((row) => row.id === move.rowId);
  if (!card) return columns;
  return columns.map((candidate) => {
    let rows: QueryRow[] = candidate.rows.filter((row) => row.id !== move.rowId);
    if (candidate.key === move.group) {
      const at = move.beforeId === null ? -1 : rows.findIndex((row) => row.id === move.beforeId);
      rows = at < 0 ? [...rows, card] : [...rows.slice(0, at), card, ...rows.slice(at)];
    }
    const removed = candidate.rows.length - candidate.rows.filter((row) => row.id !== move.rowId).length;
    const added = candidate.key === move.group ? 1 : 0;
    return { ...candidate, rows, count: candidate.count - removed + added };
  });
}

/** Keyboard alternative to dragging: one step to a neighbouring column or up and down within the column. */
export function stepCard(columns: BoardColumn[], rowId: string, direction: "left" | "right" | "up" | "down"): CardMove | null {
  const at = columns.findIndex((candidate) => candidate.rows.some((row) => row.id === rowId));
  const source = columns[at];
  if (!source) return null;
  const index = source.rows.findIndex((row) => row.id === rowId);
  if (direction === "up") return index > 0 ? planCardMove(columns, rowId, source.key, source.key, source.rows[index - 1]?.id ?? null) : null;
  if (direction === "down") {
    if (index >= source.rows.length - 1) return null;
    return planCardMove(columns, rowId, source.key, source.key, source.rows[index + 2]?.id ?? null);
  }
  const neighbour = columns[at + (direction === "left" ? -1 : 1)];
  return neighbour ? planCardMove(columns, rowId, source.key, neighbour.key, null) : null;
}
