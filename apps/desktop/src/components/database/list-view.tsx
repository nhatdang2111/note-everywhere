import { For, Show, createEffect, createMemo, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { computeWindow } from "../../lib/db/virtual-window.ts";
import { Button } from "../common/button.tsx";
import { Icon } from "../common/icons.tsx";
import { EmptyState, Spinner } from "../common/states.tsx";
import { CardBody } from "./card-body.tsx";
import { useDatabase, useDatabaseActions } from "./database-context.tsx";

const ROW_HEIGHT = 34;
const FALLBACK_VIEWPORT = 600;

/** Compact rows with the chosen properties, windowed like the table. */
export function ListView(): JSX.Element {
  const db = useDatabase();
  const actions = useDatabaseActions();
  const [scrollTop, setScrollTop] = createSignal(0);
  const [viewport, setViewport] = createSignal(0);
  const win = createMemo(() => computeWindow({ scrollTop: scrollTop(), viewportHeight: viewport() || FALLBACK_VIEWPORT, rowHeight: ROW_HEIGHT, total: db.total() }));
  createEffect(() => db.ensureRange(win().start, win().end));
  const keys = createMemo(() => Array.from({ length: Math.max(0, win().end - win().start) }, (_, offset) => win().start + offset));

  const onKeyDown = (event: KeyboardEvent) => {
    const items = [...(event.currentTarget as HTMLElement).querySelectorAll<HTMLElement>("[data-list-row]")];
    const at = items.indexOf(document.activeElement as HTMLElement);
    if (event.key === "ArrowDown") (event.preventDefault(), items[Math.min(at + 1, items.length - 1)]?.focus());
    else if (event.key === "ArrowUp") (event.preventDefault(), items[Math.max(at - 1, 0)]?.focus());
  };

  return (
    <Show when={!db.loading() || db.total() > 0} fallback={<Spinner />}>
      <Show when={db.total() > 0} fallback={<EmptyState icon="page" title={t("db.noRowsTitle")} action={<Button variant="primary" icon="plus" onClick={() => void db.createRow()}>{t("db.newRow")}</Button>}>{t("db.noRowsBody")}</EmptyState>}>
        <div
          class="list-scroll"
          ref={(element) => queueMicrotask(() => setViewport(element.clientHeight))}
          onScroll={(event) => (setScrollTop(event.currentTarget.scrollTop), setViewport(event.currentTarget.clientHeight))}
        >
          <ul class="list-rows" aria-label={t("db.listLabel")} style={{ height: `${win().totalHeight}px` }} onKeyDown={onKeyDown}>
            <For each={keys()}>
              {(index) => (
                <li style={{ position: "absolute", top: `${index * ROW_HEIGHT}px`, height: `${ROW_HEIGHT}px` }} class="list-item">
                  <Show when={db.rowAt(index)} fallback={<span class="cell-skeleton" aria-hidden="true" />}>
                    {(row) => (
                      <button type="button" class="list-row" data-list-row onClick={(event) => actions.openRow(row().id, row().title, event.metaKey || event.ctrlKey)}>
                        <Icon name="page" size={14} />
                        <CardBody row={row()} max={4} />
                      </button>
                    )}
                  </Show>
                </li>
              )}
            </For>
          </ul>
        </div>
        <div class="db-table-footer">
          <Button variant="ghost" icon="plus" onClick={() => void db.createRow()}>{t("db.newRow")}</Button>
          <span class="db-count">{t("db.rowCount", { count: db.total() })}</span>
        </div>
      </Show>
    </Show>
  );
}
