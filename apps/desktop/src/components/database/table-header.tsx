import { For, Show, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { MIN_COLUMN_WIDTH, TITLE_WIDTH, orderedColumns, withColumnMoved, withColumnWidth, type Column } from "../../lib/db/view-model.ts";
import { IconButton } from "../common/button.tsx";
import { Icon } from "../common/icons.tsx";
import { Menu } from "../common/menu.tsx";
import { useDatabase, useDatabaseActions } from "./database-context.tsx";
import { propertyMenuEntries } from "./property-menu.ts";

export interface TableHeaderProps {
  columns: Column[];
  /** Widths dragged but not yet saved, by property id. */
  widths: Record<string, number>;
  setWidth(propertyId: string, width: number | null): void;
}

const RESIZE_STEP = 16;

/** Column headers: name, sort mark, menu, drag to reorder, and a handle to resize with the pointer or arrow keys. */
export function TableHeader(props: TableHeaderProps): JSX.Element {
  const db = useDatabase();
  const actions = useDatabaseActions();
  const [menu, setMenu] = createSignal<{ id: string; at: { x: number; y: number } } | null>(null);
  const [dragging, setDragging] = createSignal<string | null>(null);

  const widthOf = (column: Column) => props.widths[column.property.id] ?? column.width;
  const sortOf = (id: string) => db.view()?.sorts.find((sort) => sort.property === id);

  const save = (propertyId: string, width: number) => {
    const schema = db.schema();
    const view = db.view();
    props.setWidth(propertyId, null);
    if (schema && view) void db.saveView(withColumnWidth(schema, view, propertyId, width));
  };

  const startResize = (event: PointerEvent, column: Column) => {
    event.preventDefault();
    event.stopPropagation();
    const startX = event.clientX;
    const startWidth = widthOf(column);
    let width = startWidth;
    const move = (next: PointerEvent) => {
      width = Math.max(MIN_COLUMN_WIDTH, startWidth + next.clientX - startX);
      props.setWidth(column.property.id, width);
    };
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
      save(column.property.id, width);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  };

  const drop = (targetId: string) => {
    const dragged = dragging();
    setDragging(null);
    const schema = db.schema();
    const view = db.view();
    if (!dragged || dragged === targetId || !schema || !view) return;
    const index = orderedColumns(schema, view).findIndex((column) => column.property.id === targetId);
    if (index >= 0) void db.saveView(withColumnMoved(schema, view, dragged, index));
  };

  const sortMark = (id: string) => {
    const sort = sortOf(id);
    return sort ? <Icon name={sort.descending ? "arrow-down" : "arrow-up"} size={12} label={t(sort.descending ? "db.sortedDescending" : "db.sortedAscending")} /> : null;
  };

  return (
    <div class="db-table-head" role="row">
      <div class="db-head-cell" role="columnheader" style={{ width: `${TITLE_WIDTH}px` }}>
        <span class="db-head-name">{t("db.title")}</span>
        {sortMark("title")}
        <IconButton
          icon="chevron-down"
          size={12}
          label={t("db.columnMenu", { name: t("db.title") })}
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            setMenu({ id: "title", at: { x: rect.left, y: rect.bottom + 4 } });
          }}
        />
      </div>
      <For each={props.columns.map((candidate) => candidate.property.id)}>
        {(columnId) => {
          // Looked up by id so a width or order change updates the header in place and keeps keyboard focus on its handle.
          const column = () => props.columns.find((candidate) => candidate.property.id === columnId) as Column;
          return (
          <div
            class="db-head-cell"
            role="columnheader"
            data-dragging={dragging() === columnId ? "true" : undefined}
            style={{ width: `${widthOf(column())}px` }}
            draggable="true"
            title={t(`db.type.${column().property.type}`)}
            onDragStart={(event) => {
              event.dataTransfer?.setData("application/x-ne-column", columnId);
              setDragging(columnId);
            }}
            onDragEnd={() => setDragging(null)}
            onDragOver={(event) => dragging() && event.preventDefault()}
            onDrop={(event) => (event.preventDefault(), drop(columnId))}
          >
            <span class="db-head-name">{column().property.name}</span>
            {sortMark(columnId)}
            <IconButton
              icon="chevron-down"
              size={12}
              label={t("db.columnMenu", { name: column().property.name })}
              onClick={(event) => {
                const rect = event.currentTarget.getBoundingClientRect();
                setMenu({ id: columnId, at: { x: rect.left, y: rect.bottom + 4 } });
              }}
            />
            <div
              class="db-resize"
              role="separator"
              aria-orientation="vertical"
              aria-label={t("db.resizeColumn", { name: column().property.name })}
              aria-valuenow={widthOf(column())}
              tabindex="0"
              onPointerDown={(event) => startResize(event, column())}
              onKeyDown={(event) => {
                if (event.key !== "ArrowLeft" && event.key !== "ArrowRight") return;
                event.preventDefault();
                save(columnId, widthOf(column()) + (event.key === "ArrowRight" ? RESIZE_STEP : -RESIZE_STEP));
              }}
            />
          </div>
          );
        }}
      </For>
      <div class="db-head-cell db-head-add" role="columnheader">
        <IconButton icon="plus" label={t("db.addProperty")} onClick={() => actions.openSchemaDialog({ kind: "add-property" })} />
      </div>
      <Show when={menu()}>
        {(open) => {
          const property = open().id === "title" ? "title" : props.columns.find((column) => column.property.id === open().id)?.property;
          return property ? (
            <Menu label={t("db.columnMenu", { name: property === "title" ? t("db.title") : property.name })} at={open().at} entries={propertyMenuEntries(db, actions, property)} onClose={() => setMenu(null)} />
          ) : null;
        }}
      </Show>
    </div>
  );
}
