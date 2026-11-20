import { For, Show, type JSX } from "solid-js";
import type { QueryRow } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { visibleColumns } from "../../lib/db/view-model.ts";
import { CellDisplay } from "./cell-display.tsx";
import { useDatabase } from "./database-context.tsx";

/** Title and the first few filled-in visible properties of a row, for cards and list rows. */
export function CardBody(props: { row: QueryRow; max?: number; skip?: string | null }): JSX.Element {
  const db = useDatabase();
  const shown = () => {
    const schema = db.schema();
    const view = db.view();
    if (!schema || !view) return [];
    return visibleColumns(schema, view)
      .filter((column) => column.property.id !== props.skip && props.row.values[column.property.id])
      .slice(0, props.max ?? 3);
  };
  return (
    <>
      <span class="card-title">{props.row.title || t("tree.untitled")}</span>
      <Show when={shown().length > 0}>
        <span class="card-props">
          <For each={shown()}>
            {(column) => (
              <span class="card-prop" title={column.property.name}>
                <CellDisplay property={column.property} value={props.row.values[column.property.id]} titles={db.titles()} />
              </span>
            )}
          </For>
        </span>
      </Show>
    </>
  );
}
