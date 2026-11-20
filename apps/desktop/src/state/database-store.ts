import { createMemo, createSignal, onCleanup } from "solid-js";
import type { ApiError, CellValue, DatabaseSchema, NodeId, PropertyType, QueryRow, QuerySpec, ViewDef, ViewKind, OptionColor } from "../api/contract.ts";
import { toApiError } from "../api/errors.ts";
import { applyCardMove, type BoardColumn, type CardMove } from "../lib/db/board-model.ts";
import { monthGrid, planDayDrop, type MonthGrid } from "../lib/db/calendar-model.ts";
import { readJson, writeJson } from "../lib/storage.ts";
import { boardSpec, calendarSpec, flatSpec } from "../lib/db/view-spec.ts";
import { pagesFor, pageQuery } from "../lib/db/virtual-window.ts";
import type { App } from "./app.tsx";

/** Rows are fetched this many at a time; a table of 10,000 rows only ever holds the pages near the viewport. */
export const PAGE_SIZE = 200;
const BOARD_PAGE = 100;

const viewKey = (databaseId: NodeId) => `ne.db.view.${databaseId}`;
const isString = (value: unknown): value is string => typeof value === "string";

interface FlatData {
  specKey: string;
  total: number;
  pages: Record<number, QueryRow[]>;
}

export interface DatabaseController {
  id: NodeId;
  schema: () => DatabaseSchema | null;
  loading: () => boolean;
  error: () => ApiError | null;
  view: () => ViewDef | null;
  setActiveView(viewId: string): void;

  // Table and list: a window onto the whole result.
  total: () => number;
  rowAt(index: number): QueryRow | undefined;
  rowById(id: NodeId): QueryRow | undefined;
  /** Titles of the rows that relation values point at. */
  titles: () => Record<NodeId, string>;
  /** Make sure the pages covering rows `start..end` (end exclusive) are loaded. */
  ensureRange(start: number, end: number): void;

  // Board
  columns: () => BoardColumn[] | null;
  loadMore(key: string | null): Promise<void>;
  moveCard(move: CardMove): Promise<void>;

  // Calendar
  calendar: () => { grid: MonthGrid; rows: QueryRow[] };
  setMonth(year: number, month: number): void;
  moveRowToDay(rowId: NodeId, day: string): Promise<void>;

  refresh(): Promise<void>;
  /** Edits, each resolving to false after showing the failure. */
  setCell(rowId: NodeId, propertyId: string, value: CellValue | null): Promise<boolean>;
  createRow(args?: { title?: string; values?: Record<string, CellValue>; index?: number }): Promise<NodeId | null>;
  createRowFromTemplate(templateId: string): Promise<NodeId | null>;
  saveView(view: ViewDef): Promise<boolean>;
  addView(kind: ViewKind, name: string): Promise<string | null>;
  duplicateView(viewId: string): Promise<string | null>;
  deleteView(viewId: string): Promise<boolean>;
  moveView(viewId: string, index: number): Promise<boolean>;
  addProperty(name: string, type: PropertyType): Promise<string | null>;
  addRelation(name: string, targetId: NodeId, reverseName?: string): Promise<string | null>;
  renameProperty(propertyId: string, name: string): Promise<boolean>;
  deleteProperty(propertyId: string): Promise<boolean>;
  moveProperty(propertyId: string, index: number): Promise<boolean>;
  changeType(propertyId: string, type: PropertyType, relationDatabaseId?: NodeId): Promise<boolean>;
  addOption(propertyId: string, name: string, color?: OptionColor): Promise<string | null>;
  renameOption(propertyId: string, optionId: string, name: string): Promise<boolean>;
  colorOption(propertyId: string, optionId: string, color: OptionColor): Promise<boolean>;
  deleteOption(propertyId: string, optionId: string): Promise<boolean>;
  moveOption(propertyId: string, optionId: string, index: number): Promise<boolean>;
}

/**
 * Controller for one open database: schema, the active view's rows, and every edit. Create it inside a component.
 * `schemaOnly` skips the rows, for places that only need to edit the schema or one row (a row page, template defaults).
 */
export function createDatabaseController(app: App, id: NodeId, options: { schemaOnly?: boolean } = {}): DatabaseController {
  const { core } = app;
  const [schema, setSchema] = createSignal<DatabaseSchema | null>(null);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<ApiError | null>(null);
  const [activeId, setActiveId] = createSignal<string | null>(readJson<string | null>(viewKey(id), null, (value): value is string | null => value === null || isString(value)));
  const [flat, setFlat] = createSignal<FlatData>({ specKey: "", total: 0, pages: {} });
  const [columns, setColumns] = createSignal<BoardColumn[] | null>(null);
  const [calendarRows, setCalendarRows] = createSignal<QueryRow[]>([]);
  const [titles, setTitles] = createSignal<Record<NodeId, string>>({});
  const today = new Date();
  const [month, setMonthSignal] = createSignal({ year: today.getFullYear(), month: today.getMonth() });

  const view = createMemo<ViewDef | null>(() => {
    const current = schema();
    if (!current) return null;
    return current.views.find((candidate) => candidate.id === activeId()) ?? current.views[0] ?? null;
  });
  const grid = createMemo(() => monthGrid(month().year, month().month));

  const flatRows = createMemo(() => new Map(Object.values(flat().pages).flatMap((page) => page.map((row) => [row.id, row] as const))));

  const specFor = (current: DatabaseSchema, active: ViewDef): QuerySpec => {
    if (active.kind === "board") return boardSpec(current, active);
    if (active.kind === "calendar") return calendarSpec(current, active, grid().from, grid().to);
    return flatSpec(current, active);
  };

  let generation = 0;
  const inFlight = new Set<number>();
  const wanted = new Set<number>([0]);

  const fail = (failure: unknown) => {
    setError(toApiError(failure));
    setLoading(false);
  };

  async function loadFlat(current: DatabaseSchema, active: ViewDef, pages: number[], gen: number): Promise<void> {
    const spec = flatSpec(current, active);
    const key = JSON.stringify(spec);
    const results = await Promise.all(pages.map((page) => core.api.dbQuery({ id, spec, ...pageQuery(page, PAGE_SIZE) })));
    if (gen !== generation) return;
    setFlat((previous) => {
      const base = previous.specKey === key ? previous.pages : {};
      const next = { ...base };
      pages.forEach((page, at) => (next[page] = results[at]?.rows ?? []));
      return { specKey: key, total: results[0]?.total ?? 0, pages: next };
    });
    setTitles((previous) => Object.assign({}, previous, ...results.map((result) => result.titles)));
  }

  /** Re-read the schema and whatever the active view shows; what is on screen stays until the new data arrives. */
  async function refresh(): Promise<void> {
    const gen = ++generation;
    try {
      const current = await core.api.dbSchema({ id });
      if (gen !== generation) return;
      setSchema(current);
      const active = current.views.find((candidate) => candidate.id === activeId()) ?? current.views[0];
      if (!active || options.schemaOnly) {
        setLoading(false);
        setError(null);
        return;
      }
      if (active.kind === "board") {
        const result = await core.api.dbQuery({ id, spec: { ...boardSpec(current, active), limit: BOARD_PAGE } });
        if (gen !== generation) return;
        setColumns(result.groups);
        setTitles(result.titles);
      } else if (active.kind === "calendar") {
        const result = await core.api.dbQuery({ id, spec: specFor(current, active) });
        if (gen !== generation) return;
        setCalendarRows(result.rows);
        setTitles(result.titles);
      } else {
        const spec = flatSpec(current, active);
        const same = flat().specKey === JSON.stringify(spec);
        await loadFlat(current, active, same ? [...wanted] : [0], gen);
        if (!same) {
          wanted.clear();
          wanted.add(0);
        }
      }
      if (gen === generation) {
        setError(null);
        setLoading(false);
      }
    } catch (failure) {
      if (gen === generation) fail(failure);
    }
  }

  let scheduled = false;
  const scheduleRefresh = () => {
    if (scheduled) return;
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      void refresh();
    });
  };

  // This device's own edits arrive as `database:changed`; other devices' as `tree:changed` naming the database or its rows.
  let stopEvents: (() => void)[] = [];
  let disposed = false;
  void core.events.on("database:changed", ({ databaseId }) => databaseId === id && scheduleRefresh()).then((stop) => (disposed ? stop() : stopEvents.push(stop)));
  void core.events
    .on("tree:changed", ({ nodeIds }) => {
      if (nodeIds.includes(id) || nodeIds.some((nodeId) => flatRows().has(nodeId))) scheduleRefresh();
    })
    .then((stop) => (disposed ? stop() : stopEvents.push(stop)));
  onCleanup(() => {
    disposed = true;
    stopEvents.forEach((stop) => stop());
    stopEvents = [];
  });
  void refresh();

  async function run<T>(action: () => Promise<T>, fallback: T): Promise<T> {
    try {
      const result = await action();
      await refresh();
      return result;
    } catch (failure) {
      app.toasts.error(failure);
      void refresh();
      return fallback;
    }
  }

  const needView = (viewId: string): ViewDef | undefined => schema()?.views.find((candidate) => candidate.id === viewId);

  const patchCache = (rowId: NodeId, propertyId: string, value: CellValue | null) => {
    const edit = (row: QueryRow): QueryRow => {
      if (row.id !== rowId) return row;
      if (propertyId === "title") return { ...row, title: value?.type === "text" ? value.value : "" };
      const values = { ...row.values };
      if (value === null) delete values[propertyId];
      else values[propertyId] = value;
      return { ...row, values };
    };
    setFlat((previous) => ({ ...previous, pages: Object.fromEntries(Object.entries(previous.pages).map(([page, rows]) => [page, rows.map(edit)])) }));
    setCalendarRows((rows) => rows.map(edit));
    setColumns((current) => current && current.map((column) => ({ ...column, rows: column.rows.map(edit) })));
  };

  return {
    id,
    schema,
    loading,
    error,
    view,
    setActiveView(viewId) {
      if (viewId === activeId()) return;
      setActiveId(viewId);
      writeJson(viewKey(id), viewId);
      wanted.clear();
      wanted.add(0);
      setLoading(true);
      void refresh();
    },
    total: () => flat().total,
    rowAt: (index) => flat().pages[Math.floor(index / PAGE_SIZE)]?.[index % PAGE_SIZE],
    rowById: (rowId) => flatRows().get(rowId),
    titles,
    ensureRange(start, end) {
      const current = schema();
      const active = view();
      if (!current || !active || active.kind === "board" || active.kind === "calendar") return;
      const missing = pagesFor(start, Math.min(end, flat().total), PAGE_SIZE).filter((page) => !(page in flat().pages) && !inFlight.has(page));
      if (missing.length === 0) return;
      missing.forEach((page) => (inFlight.add(page), wanted.add(page)));
      void loadFlat(current, active, missing, generation)
        .catch(fail)
        .finally(() => missing.forEach((page) => inFlight.delete(page)));
    },

    columns,
    async loadMore(key) {
      const current = schema();
      const active = view();
      const column = columns()?.find((candidate) => candidate.key === key);
      if (!current || !active || !column) return;
      try {
        const result = await core.api.dbQuery({ id, spec: { ...boardSpec(current, active), offset: column.rows.length, limit: BOARD_PAGE } });
        const more = result.groups.find((candidate) => candidate.key === key)?.rows ?? [];
        setColumns((previous) => previous && previous.map((candidate) => (candidate.key === key ? { ...candidate, rows: [...candidate.rows, ...more] } : candidate)));
        setTitles((previous) => ({ ...previous, ...result.titles }));
      } catch (failure) {
        app.toasts.error(failure);
      }
    },
    async moveCard(move) {
      const group = view()?.group;
      if (!group) return;
      setColumns((current) => (current ? applyCardMove(current, move) : current));
      await run(() => core.api.dbCardMove({ id, rowId: move.rowId, propertyId: group.property, group: move.group, beforeId: move.beforeId }), undefined);
    },

    calendar: () => ({ grid: grid(), rows: calendarRows() }),
    setMonth(year, monthIndex) {
      setMonthSignal({ year, month: monthIndex });
      void refresh();
    },
    async moveRowToDay(rowId, day) {
      const property = view()?.dateProperty;
      const value = property ? planDayDrop(calendarRows(), rowId, property, day) : null;
      if (!property || !value) return;
      patchCache(rowId, property, { type: "date", value });
      await run(() => core.api.dbCellSet({ id, rowId, propertyId: property, value: { type: "date", value } }), undefined);
    },

    refresh,
    async setCell(rowId, propertyId, value) {
      patchCache(rowId, propertyId, value);
      return run(async () => {
        await core.api.dbCellSet({ id, rowId, propertyId, value });
        if (propertyId === "title") app.rows.retitle(rowId, value?.type === "text" ? value.value : "");
        return true;
      }, false);
    },
    createRow: (args) => run(() => core.api.dbRowCreate({ id, ...args }), null),
    createRowFromTemplate: (templateId) => run(() => core.api.templateInstantiateRow({ id: templateId }), null),
    saveView(next) {
      setSchema((current) => current && { ...current, views: current.views.map((candidate) => (candidate.id === next.id ? next : candidate)) });
      return run(async () => (await core.api.dbViewSave({ id, view: next }), true), false);
    },
    async addView(kind, name) {
      const viewId = await run(() => core.api.dbViewAdd({ id, name, kind }), null);
      if (viewId) this.setActiveView(viewId);
      return viewId;
    },
    async duplicateView(viewId) {
      const source = needView(viewId);
      if (!source) return null;
      const copyId = await run(async () => {
        const created = await core.api.dbViewAdd({ id, name: `${source.name} (copy)`, kind: source.kind });
        await core.api.dbViewSave({ id, view: { ...structuredClone(source), id: created, name: `${source.name} (copy)` } });
        return created;
      }, null);
      if (copyId) this.setActiveView(copyId);
      return copyId;
    },
    async deleteView(viewId) {
      const ok = await run(async () => (await core.api.dbViewDelete({ id, viewId }), true), false);
      if (ok && activeId() === viewId) {
        setActiveId(null);
        writeJson(viewKey(id), null);
        await refresh();
      }
      return ok;
    },
    moveView: (viewId, index) => run(async () => (await core.api.dbViewMove({ id, viewId, index }), true), false),
    addProperty: (name, type) => run(() => core.api.dbPropertyAdd({ id, name, type }), null),
    addRelation: (name, targetId, reverseName) => run(() => core.api.dbRelationAdd({ id, name, targetId, reverseName }), null),
    renameProperty: (propertyId, name) => run(async () => (await core.api.dbPropertyRename({ id, propertyId, name }), true), false),
    deleteProperty: (propertyId) => run(async () => (await core.api.dbPropertyDelete({ id, propertyId }), true), false),
    moveProperty: (propertyId, index) => run(async () => (await core.api.dbPropertyMove({ id, propertyId, index }), true), false),
    changeType: (propertyId, type, relationDatabaseId) =>
      run(async () => (await core.api.dbPropertyChangeType({ id, propertyId, type, relationDatabaseId }), true), false),
    addOption: (propertyId, name, color) => run(() => core.api.dbOptionAdd({ id, propertyId, name, color }), null),
    renameOption: (propertyId, optionId, name) => run(async () => (await core.api.dbOptionRename({ id, propertyId, optionId, name }), true), false),
    colorOption: (propertyId, optionId, color) => run(async () => (await core.api.dbOptionColor({ id, propertyId, optionId, color }), true), false),
    deleteOption: (propertyId, optionId) => run(async () => (await core.api.dbOptionDelete({ id, propertyId, optionId }), true), false),
    moveOption: (propertyId, optionId, index) => run(async () => (await core.api.dbOptionMove({ id, propertyId, optionId, index }), true), false),
  };
}
