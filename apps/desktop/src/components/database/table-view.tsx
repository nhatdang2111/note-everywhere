import { For, Show, createEffect, createMemo, createSignal, type JSX } from "solid-js";
import type { CellValue, DbProperty, QueryRow } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { cellToText, parseCellText } from "../../lib/db/cell-convert.ts";
import { moveCell, type CellPos } from "../../lib/db/table-nav.ts";
import { TITLE_WIDTH, visibleColumns, type Column } from "../../lib/db/view-model.ts";
import { computeWindow, scrollToReveal } from "../../lib/db/virtual-window.ts";
import { Button } from "../common/button.tsx";
import { Icon } from "../common/icons.tsx";
import { Menu, type MenuEntry } from "../common/menu.tsx";
import { EmptyState, Spinner } from "../common/states.tsx";
import type { PropertyRef } from "./cell-display.tsx";
import { useDatabase, useDatabaseActions } from "./database-context.tsx";
import { EditableCell, isTextLike, startingText, type EditExit } from "./editable-cell.tsx";
import { TableHeader } from "./table-header.tsx";

export const ROW_HEIGHT = 36;
/** Used where the browser reports no height (before layout, or in tests). */
const FALLBACK_VIEWPORT = 600;
const CELL_MIME = "application/x-ne-cell";

interface Editing {
  rowId: string;
  propertyId: string;
  text: string;
}

/** The rows of a database as a grid: only the rows near the viewport exist in the page, and the rest are fetched as you scroll. */
export function TableView(): JSX.Element {
  const app = useApp();
  const db = useDatabase();
  const actions = useDatabaseActions();
  let scroller!: HTMLDivElement;
  let grid!: HTMLDivElement;

  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewport, setViewport] = createSignal(0);
  const [widths, setWidths] = createSignal<Record<string, number>>({});
  const [active, setActive] = createSignal<CellPos | null>(null);
  const [editing, setEditing] = createSignal<Editing | null>(null);
  const [menu, setMenu] = createSignal<{ rowId: string; title: string; at: { x: number; y: number } } | null>(null);

  const columns = createMemo<Column[]>(() => {
    const schema = db.schema();
    const view = db.view();
    return schema && view ? visibleColumns(schema, view) : [];
  });
  const colCount = () => columns().length + 1;
  const propertyAt = (col: number): PropertyRef => (col === 0 ? "title" : ((columns()[col - 1] as Column).property as DbProperty));
  const widthAt = (col: number) => {
    if (col === 0) return TITLE_WIDTH;
    const column = columns()[col - 1] as Column;
    return widths()[column.property.id] ?? column.width;
  };
  const rowWidth = () => Array.from({ length: colCount() }, (_, col) => widthAt(col)).reduce((a, b) => a + b, 0) + 44;
  const propertyId = (col: number) => (propertyAt(col) === "title" ? "title" : (propertyAt(col) as DbProperty).id);

  const win = createMemo(() => computeWindow({ scrollTop: scrollTop(), viewportHeight: viewport() || FALLBACK_VIEWPORT, rowHeight: ROW_HEIGHT, total: db.total() }));
  createEffect(() => db.ensureRange(win().start, win().end));

  // Rows are keyed by id so a refresh updates cells in place; a row still being fetched keeps a placeholder key.
  const keys = createMemo(() => {
    const { start, end } = win();
    return Array.from({ length: Math.max(0, end - start) }, (_, offset) => db.rowAt(start + offset)?.id ?? `~${start + offset}`);
  });
  const indexOfKey = createMemo(() => new Map(keys().map((key, offset) => [key, win().start + offset] as const)));

  const valueOf = (row: QueryRow | undefined, col: number): CellValue | undefined => {
    const property = propertyAt(col);
    return row && property !== "title" ? row.values[property.id] : undefined;
  };

  const cellElement = (row: number, col: number) => grid.querySelector<HTMLElement>(`[data-row="${row}"][data-col="${col}"]`);

  /** Select a cell, bring it into view and put focus on it once it is drawn. */
  const select = (pos: CellPos, focus = true) => {
    setActive(pos);
    const next = scrollToReveal(pos.row, ROW_HEIGHT, scroller.scrollTop, viewport() || FALLBACK_VIEWPORT);
    if (next !== scroller.scrollTop) {
      scroller.scrollTop = next;
      setScrollTop(next);
    }
    if (focus) queueMicrotask(() => cellElement(pos.row, pos.col)?.focus());
  };

  const startEdit = (pos: CellPos, seed?: string) => {
    const row = db.rowAt(pos.row);
    const property = propertyAt(pos.col);
    if (!row || (property !== "title" && property.type === "checkbox")) return;
    if (property !== "title" && property.relation?.backlinkOf && property.type !== "relation") return;
    setActive(pos);
    setEditing({ rowId: row.id, propertyId: propertyId(pos.col), text: seed ?? startingText(property, valueOf(row, pos.col), row.title) });
  };

  const save = (row: QueryRow, col: number, value: CellValue | null) => void db.setCell(row.id, propertyId(col), value);

  const toggleCheckbox = (row: QueryRow, col: number) => {
    const current = valueOf(row, col);
    save(row, col, { type: "checkbox", value: !(current?.type === "checkbox" && current.value) });
  };

  const finishEdit = (pos: CellPos, exit: EditExit) => {
    setEditing(null);
    if (exit === "blur") return;
    const last = Math.max(db.total() - 1, 0);
    let target: CellPos = pos;
    if (exit === "enter") target = { row: Math.min(pos.row + 1, last), col: pos.col };
    else if (exit === "tab") target = pos.col + 1 < colCount() ? { row: pos.row, col: pos.col + 1 } : { row: Math.min(pos.row + 1, last), col: 0 };
    else if (exit === "shift-tab") target = pos.col > 0 ? { row: pos.row, col: pos.col - 1 } : { row: Math.max(pos.row - 1, 0), col: colCount() - 1 };
    select(target);
  };

  const openRowAt = (index: number, inNewTab = false) => {
    const row = db.rowAt(index);
    if (row) actions.openRow(row.id, row.title, inNewTab);
  };

  const openMenuAt = (index: number, x: number, y: number) => {
    const row = db.rowAt(index);
    if (row) setMenu({ rowId: row.id, title: row.title, at: { x, y } });
  };

  const menuEntries = (rowId: string, title: string): MenuEntry[] => [
    { kind: "item", icon: "page", label: t("tree.open"), onSelect: () => actions.openRow(rowId, title) },
    { kind: "item", icon: "plus", label: t("tree.openInNewTab"), onSelect: () => actions.openRow(rowId, title, true) },
    { kind: "separator" },
    { kind: "item", icon: "trash", danger: true, label: t("db.trashRow"), onSelect: () => void actions.trashRow(rowId) },
  ];

  const onKeyDown = (event: KeyboardEvent) => {
    // Keys on the header's handles and menu buttons are theirs, not cell navigation.
    if (editing() || !(event.target as HTMLElement).closest('[role="gridcell"]')) return;
    const pos = active() ?? { row: 0, col: 0 };
    const row = db.rowAt(pos.row);
    const mod = event.metaKey || event.ctrlKey;
    const size = { rows: db.total(), cols: colCount(), page: Math.max(1, Math.floor((viewport() || FALLBACK_VIEWPORT) / ROW_HEIGHT) - 1) };
    const next = moveCell(pos, event.key, size, mod);
    if (next) return (event.preventDefault(), select(next));
    if (!row) return;
    const property = propertyAt(pos.col);
    const checkbox = property !== "title" && property.type === "checkbox";
    if ((event.key === "Enter" && (mod || event.shiftKey)) || (event.key === " " && event.shiftKey)) return (event.preventDefault(), openRowAt(pos.row, mod && event.shiftKey));
    if (event.key === "Enter" || event.key === "F2" || (event.key === " " && !mod)) {
      event.preventDefault();
      return checkbox ? toggleCheckbox(row, pos.col) : startEdit(pos);
    }
    if (event.key === "Delete" || event.key === "Backspace") {
      event.preventDefault();
      if (property !== "title" && !property.relation?.backlinkOf) save(row, pos.col, null);
      return;
    }
    if (event.key === "ContextMenu" || (event.key === "F10" && event.shiftKey)) {
      event.preventDefault();
      const rect = (event.target as HTMLElement).getBoundingClientRect();
      return openMenuAt(pos.row, rect.left, rect.bottom);
    }
    if (event.key.length === 1 && !mod && !event.altKey && isTextLike(property)) {
      event.preventDefault();
      startEdit(pos, event.key);
    }
  };

  const onCopy = (event: ClipboardEvent) => {
    if (editing() || !active()) return;
    const pos = active() as CellPos;
    const row = db.rowAt(pos.row);
    const property = propertyAt(pos.col);
    if (!row || !event.clipboardData) return;
    const value = valueOf(row, pos.col);
    event.preventDefault();
    event.clipboardData.setData("text/plain", property === "title" ? row.title : cellToText(property, value, db.titles()));
    if (value) event.clipboardData.setData(CELL_MIME, JSON.stringify(value));
  };

  /** Text and the exact value copied from a cell of the same type are both understood. */
  const onPaste = async (event: ClipboardEvent) => {
    if (editing() || !active() || !event.clipboardData) return;
    const pos = active() as CellPos;
    const row = db.rowAt(pos.row);
    const property = propertyAt(pos.col);
    if (!row) return;
    event.preventDefault();
    const text = event.clipboardData.getData("text/plain");
    const raw = event.clipboardData.getData(CELL_MIME);
    if (property === "title") return void save(row, pos.col, { type: "text", value: text.trim() });
    if (property.type === "checkbox") return void save(row, pos.col, { type: "checkbox", value: /^(true|yes|1|x)$/i.test(text.trim()) });
    let exact: CellValue | undefined;
    try {
      const parsed = raw ? (JSON.parse(raw) as CellValue) : undefined;
      if (parsed?.type === property.type) exact = parsed;
    } catch {
      exact = undefined;
    }
    let value: CellValue | undefined = exact ?? parseCellText(property, text);
    if (!value && text.trim() && (property.type === "select" || property.type === "multi_select")) {
      const names = property.type === "select" ? [text.trim()] : text.split(",").map((part) => part.trim()).filter(Boolean);
      const ids: string[] = [];
      for (const name of names) {
        const id = await db.addOption(property.id, name);
        if (id) ids.push(id);
      }
      value = ids.length === 0 ? undefined : property.type === "select" ? { type: "select", value: ids[0] as string } : { type: "multi_select", value: ids };
    }
    if (value) save(row, pos.col, value);
    else if (text.trim()) app.toasts.push("info", t("db.pasteMismatch", { name: property.name }));
  };

  return (
    <div class="db-table">
      <Show when={!db.loading() || db.total() > 0} fallback={<Spinner />}>
        <Show
          when={db.total() > 0}
          fallback={
            <EmptyState icon="page" title={db.view()?.filter ? t("db.noMatchTitle") : t("db.noRowsTitle")} action={<Button variant="primary" icon="plus" onClick={() => void db.createRow()}>{t("db.newRow")}</Button>}>
              {db.view()?.filter ? t("db.noMatchBody") : t("db.noRowsBody")}
            </EmptyState>
          }
        >
          <div
            class="db-table-scroll"
            onScroll={(event) => {
              setScrollTop(event.currentTarget.scrollTop);
              setViewport(event.currentTarget.clientHeight);
            }}
            ref={(element) => {
              scroller = element;
              queueMicrotask(() => setViewport(element.clientHeight));
            }}
          >
            <div
              ref={grid}
              class="db-grid"
              role="grid"
              aria-label={t("db.tableLabel")}
              aria-rowcount={db.total() + 1}
              aria-colcount={colCount()}
              style={{ width: `${rowWidth()}px` }}
              onKeyDown={onKeyDown}
              onCopy={onCopy}
              onPaste={(event) => void onPaste(event)}
            >
              <TableHeader columns={columns()} widths={widths()} setWidth={(id, width) => setWidths((current) => (width === null ? Object.fromEntries(Object.entries(current).filter(([key]) => key !== id)) : { ...current, [id]: width }))} />
              <div class="db-table-body" role="rowgroup" style={{ height: `${win().totalHeight}px` }}>
                <div class="db-table-window" style={{ transform: `translateY(${win().offsetTop}px)` }}>
                  <For each={keys()}>
                    {(key) => {
                      const index = () => indexOfKey().get(key) ?? -1;
                      const row = () => (index() >= 0 ? db.rowAt(index()) : undefined);
                      return (
                        <div class="db-row" role="row" aria-rowindex={index() + 2} style={{ height: `${ROW_HEIGHT}px` }} data-loading={row() ? undefined : "true"} onContextMenu={(event) => {
                          if (!row()) return;
                          event.preventDefault();
                          openMenuAt(index(), event.clientX, event.clientY);
                        }}>
                          <For each={Array.from({ length: colCount() }, (_, col) => col)}>
                            {(col) => {
                              let cell!: HTMLDivElement;
                              const property = () => propertyAt(col);
                              const isActive = () => (active() ?? { row: 0, col: 0 }).row === index() && (active() ?? { row: 0, col: 0 }).col === col;
                              const isEditing = () => {
                                const current = editing();
                                return current !== null && current.rowId === row()?.id && current.propertyId === propertyId(col);
                              };
                              const checkbox = () => property() !== "title" && (property() as DbProperty).type === "checkbox";
                              return (
                                <div
                                  ref={cell}
                                  class="db-cell"
                                  role="gridcell"
                                  data-row={index()}
                                  data-col={col}
                                  data-editing={isEditing() ? "true" : undefined}
                                  aria-selected={isActive()}
                                  aria-readonly={property() !== "title" && (property() as DbProperty).relation?.backlinkOf && (property() as DbProperty).type !== "relation" ? "true" : undefined}
                                  tabindex={isActive() ? 0 : -1}
                                  style={{ width: `${widthAt(col)}px` }}
                                  onFocus={() => !isActive() && index() >= 0 && setActive({ row: index(), col })}
                                  onMouseDown={() => index() >= 0 && !isEditing() && setActive({ row: index(), col })}
                                  onDblClick={() => startEdit({ row: index(), col })}
                                  onClick={() => {
                                    const current = row();
                                    if (current && !checkbox() && !isTextLike(property()) && isActive() && !isEditing()) startEdit({ row: index(), col });
                                  }}
                                >
                                  <Show when={row()} fallback={<span class="cell-skeleton" aria-hidden="true" />}>
                                    {(current) => (
                                      <>
                                        <Show when={col === 0 && !isEditing()}>
                                          <button
                                            type="button"
                                            class="db-open-row"
                                            tabindex="-1"
                                            aria-label={t("db.openRow", { title: current().title || t("tree.untitled") })}
                                            title={t("db.openRow", { title: current().title || t("tree.untitled") })}
                                            onClick={(event) => (event.stopPropagation(), openRowAt(index()))}
                                          >
                                            <Icon name="page" size={14} />
                                          </button>
                                        </Show>
                                        <Show
                                          when={checkbox()}
                                          fallback={
                                            <EditableCell
                                              property={property()}
                                              value={valueOf(current(), col)}
                                              title={current().title}
                                              titles={db.titles()}
                                              editing={isEditing()}
                                              text={editing()?.text}
                                              onText={(text) => setEditing((state) => (state ? { ...state, text } : state))}
                                              onChange={(value) => save(current(), col, value)}
                                              onExit={(exit) => finishEdit({ row: index(), col }, exit)}
                                              anchor={() => cell}
                                              onManageOptions={() => property() !== "title" && actions.openSchemaDialog({ kind: "options", propertyId: (property() as DbProperty).id })}
                                            />
                                          }
                                        >
                                          <input
                                            type="checkbox"
                                            class="cell-checkbox"
                                            tabindex="-1"
                                            aria-label={(property() as DbProperty).name}
                                            checked={valueOf(current(), col)?.type === "checkbox" && (valueOf(current(), col) as { value: boolean }).value}
                                            onChange={() => toggleCheckbox(current(), col)}
                                          />
                                        </Show>
                                      </>
                                    )}
                                  </Show>
                                </div>
                              );
                            }}
                          </For>
                        </div>
                      );
                    }}
                  </For>
                </div>
              </div>
            </div>
          </div>
          <div class="db-table-footer">
            <Button variant="ghost" icon="plus" onClick={() => void db.createRow()}>
              {t("db.newRow")}
            </Button>
            <span class="db-count" aria-live="polite">
              {t("db.rowCount", { count: db.total() })}
            </span>
          </div>
        </Show>
      </Show>
      <Show when={menu()}>{(open) => <Menu label={t("db.rowMenu")} at={open().at} entries={menuEntries(open().rowId, open().title)} onClose={() => setMenu(null)} />}</Show>
    </div>
  );
}
