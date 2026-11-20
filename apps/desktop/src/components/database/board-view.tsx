import { For, Show, createSignal, type JSX } from "solid-js";
import type { CellValue, DbProperty } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { planCardMove, stepCard, type BoardColumn } from "../../lib/db/board-model.ts";
import { GROUPABLE } from "../../lib/db/operators.ts";
import { Button } from "../common/button.tsx";
import { EmptyState, Spinner } from "../common/states.tsx";
import { CardBody } from "./card-body.tsx";
import { useDatabase, useDatabaseActions } from "./database-context.tsx";
import { OptionChip } from "./option-chip.tsx";

interface Drag {
  rowId: string;
  from: string | null;
}

/** Cards in columns by a select, multi-select or checkbox property; drag a card, or use Alt+arrows, to move it. */
export function BoardView(): JSX.Element {
  const db = useDatabase();
  const actions = useDatabaseActions();
  const [drag, setDrag] = createSignal<Drag | null>(null);
  const [over, setOver] = createSignal<{ key: string | null; beforeId: string | null } | null>(null);
  const [adding, setAdding] = createSignal<string | null | undefined>(undefined);
  let board!: HTMLDivElement;

  const groupProperty = (): DbProperty | undefined => {
    const id = db.view()?.group?.property;
    return db.schema()?.properties.find((prop) => prop.id === id);
  };
  const groupable = () => (db.schema()?.properties ?? []).filter((prop) => GROUPABLE.includes(prop.type) && !prop.relation?.backlinkOf);

  const labelOf = (column: BoardColumn): JSX.Element => {
    const property = groupProperty();
    if (column.key === null) return <span class="board-none">{t("db.noValue")}</span>;
    if (property?.type === "checkbox") return <span>{t(column.key === "true" ? "db.checked" : "db.unchecked")}</span>;
    const option = property?.options.find((candidate) => candidate.id === column.key);
    return option ? <OptionChip name={option.name} color={option.color} /> : <span>{column.key}</span>;
  };
  const textOf = (column: BoardColumn): string => {
    const property = groupProperty();
    if (column.key === null) return t("db.noValue");
    if (property?.type === "checkbox") return t(column.key === "true" ? "db.checked" : "db.unchecked");
    return property?.options.find((candidate) => candidate.id === column.key)?.name ?? column.key;
  };

  const dropOn = (key: string | null, beforeId: string | null) => {
    const dragged = drag();
    setDrag(null);
    setOver(null);
    const columns = db.columns();
    if (!dragged || !columns) return;
    const move = planCardMove(columns, dragged.rowId, dragged.from, key, beforeId);
    if (move) void db.moveCard(move);
  };

  const focusCard = (rowId: string) => queueMicrotask(() => board.querySelector<HTMLElement>(`[data-card="${rowId}"]`)?.focus());

  const onCardKey = (event: KeyboardEvent, rowId: string, title: string) => {
    if (event.key === "Enter") return (event.preventDefault(), actions.openRow(rowId, title, event.metaKey || event.ctrlKey));
    const direction = ({ ArrowLeft: "left", ArrowRight: "right", ArrowUp: "up", ArrowDown: "down" } as Record<string, "left" | "right" | "up" | "down">)[event.key];
    if (!direction) return;
    event.preventDefault();
    const columns = db.columns();
    if (!columns) return;
    if (event.altKey) {
      const move = stepCard(columns, rowId, direction);
      if (move) (void db.moveCard(move), focusCard(rowId));
      return;
    }
    const cards = [...board.querySelectorAll<HTMLElement>("[data-card]")];
    const at = cards.indexOf(event.currentTarget as HTMLElement);
    const column = (event.currentTarget as HTMLElement).closest<HTMLElement>("[data-column]");
    if (direction === "up" || direction === "down") cards[at + (direction === "up" ? -1 : 1)]?.focus();
    else {
      const sibling = (direction === "left" ? column?.previousElementSibling : column?.nextElementSibling) as HTMLElement | null;
      sibling?.querySelector<HTMLElement>("[data-card]")?.focus();
    }
  };

  const addCard = async (column: BoardColumn, title: string) => {
    const property = groupProperty();
    if (!property || !title.trim()) return setAdding(undefined);
    let value: CellValue | undefined;
    if (column.key !== null) {
      if (property.type === "checkbox") value = { type: "checkbox", value: column.key === "true" };
      else if (property.type === "multi_select") value = { type: "multi_select", value: [column.key] };
      else value = { type: "select", value: column.key };
    }
    await db.createRow({ title: title.trim(), values: value ? { [property.id]: value } : undefined });
  };

  return (
    <div class="board" ref={board}>
      <Show when={db.view()?.group && groupProperty()} fallback={
        <EmptyState icon="toggle" title={t("db.boardNeedsGroup")} action={
          <Show when={groupable().length === 0}>
            <Button variant="primary" onClick={() => actions.openSchemaDialog({ kind: "add-property" })}>{t("db.addProperty")}</Button>
          </Show>
        }>
          {groupable().length > 0 ? t("db.boardPickGroup") : t("db.boardNoGroupable")}
        </EmptyState>
      }>
        <Show when={db.columns()} fallback={<Spinner />}>
          {/* Columns and cards are drawn by key, so a refresh updates them in place and keeps keyboard focus on a card. */}
          {(columns) => (
            <For each={columns().map((candidate) => candidate.key ?? "")}>
              {(columnKey) => {
                const column = () => columns().find((candidate) => (candidate.key ?? "") === columnKey) as BoardColumn;
                return (
                <section
                  class="board-column"
                  data-column={columnKey}
                  data-over={over()?.key === column().key && over()?.beforeId === null ? "true" : undefined}
                  aria-label={t("db.columnLabel", { name: textOf(column()), count: column().count })}
                  onDragOver={(event) => {
                    if (!drag()) return;
                    event.preventDefault();
                    setOver({ key: column().key, beforeId: null });
                  }}
                  onDrop={(event) => (event.preventDefault(), dropOn(column().key, over()?.key === column().key ? over()?.beforeId ?? null : null))}
                >
                  <header class="board-column-head">
                    {labelOf(column())}
                    <span class="board-count">{column().count}</span>
                  </header>
                  <ul class="board-cards">
                    <For each={column().rows.map((candidate) => candidate.id)}>
                      {(rowId) => {
                        const row = () => column().rows.find((candidate) => candidate.id === rowId);
                        return (
                        <Show when={row()}>
                          {(current) => (
                            <li>
                              <div
                                class="card"
                                role="button"
                                tabindex="0"
                                data-card={rowId}
                                data-dragging={drag()?.rowId === rowId ? "true" : undefined}
                                data-before={over()?.beforeId === rowId ? "true" : undefined}
                                aria-roledescription={t("db.card")}
                                aria-keyshortcuts="Alt+ArrowLeft Alt+ArrowRight Alt+ArrowUp Alt+ArrowDown"
                                draggable="true"
                                onClick={(event) => actions.openRow(rowId, current().title, event.metaKey || event.ctrlKey)}
                                onKeyDown={(event) => onCardKey(event, rowId, current().title)}
                                onDragStart={(event) => {
                                  event.dataTransfer?.setData("application/x-ne-card", rowId);
                                  setDrag({ rowId, from: column().key });
                                }}
                                onDragEnd={() => (setDrag(null), setOver(null))}
                                onDragOver={(event) => {
                                  if (!drag()) return;
                                  event.preventDefault();
                                  event.stopPropagation();
                                  setOver({ key: column().key, beforeId: rowId });
                                }}
                                onDrop={(event) => (event.preventDefault(), event.stopPropagation(), dropOn(column().key, rowId))}
                              >
                                <CardBody row={current()} skip={groupProperty()?.id} />
                              </div>
                            </li>
                          )}
                        </Show>
                        );
                      }}
                    </For>
                  </ul>
                  <Show when={column().count > column().rows.length}>
                    <button type="button" class="link-button board-more" onClick={() => void db.loadMore(column().key)}>
                      {t("db.loadMore", { count: column().count - column().rows.length })}
                    </button>
                  </Show>
                  <Show
                    when={adding() === column().key}
                    fallback={
                      <button type="button" class="board-add" onClick={() => setAdding(column().key)}>
                        {t("db.addCard")}
                        <span class="visually-hidden"> {textOf(column())}</span>
                      </button>
                    }
                  >
                    <input
                      class="board-add-input"
                      data-autofocus
                      ref={(element) => queueMicrotask(() => element.focus())}
                      aria-label={t("db.newCardTitle", { name: textOf(column()) })}
                      placeholder={t("db.newCardPlaceholder")}
                      onKeyDown={(event) => {
                        if (event.key === "Enter") {
                          event.preventDefault();
                          const input = event.currentTarget;
                          void addCard(column(), input.value).then(() => (input.value = ""));
                        } else if (event.key === "Escape") (event.stopPropagation(), setAdding(undefined));
                      }}
                      onBlur={(event) => event.currentTarget.value.trim() ? void addCard(column(), event.currentTarget.value).then(() => setAdding(undefined)) : setAdding(undefined)}
                    />
                  </Show>
                </section>
                );
              }}
            </For>
          )}
        </Show>
      </Show>
    </div>
  );
}
