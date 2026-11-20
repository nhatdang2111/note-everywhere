import { For, Show, createSignal, onCleanup, onMount, type JSX } from "solid-js";
import type { NodeId, QueryRow } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { createDatabaseController } from "../../state/database-store.ts";
import { loadRowValues } from "../../state/row-values.ts";
import { useApp } from "../../state/app.tsx";
import { IconButton } from "../common/button.tsx";
import { DatabaseActionsProvider, DatabaseProvider, type SchemaDialog } from "./database-context.tsx";
import { OptionsDialog } from "./options-dialog.tsx";
import { PropertyField } from "./property-field.tsx";

/** The properties of a row, shown above the row's page content. */
export function RowProperties(props: { databaseId: NodeId; rowId: NodeId }): JSX.Element {
  const app = useApp();
  const controller = createDatabaseController(app, props.databaseId, { schemaOnly: true });
  const [row, setRow] = createSignal<QueryRow | null>(null);
  const [titles, setTitles] = createSignal<Record<NodeId, string>>({});
  const [dialog, setDialog] = createSignal<SchemaDialog | null>(null);
  const properties = () => (controller.schema()?.properties ?? []).filter((prop) => !prop.relation?.backlinkOf || prop.type === "relation");

  async function reload() {
    try {
      const { row: found, titles: known } = await loadRowValues(app.core, props.databaseId, props.rowId, app.pageTitle(props.rowId));
      if (found) setRow(found);
      setTitles(known);
    } catch (error) {
      app.toasts.error(error);
    }
  }

  onMount(() => {
    void reload();
    const stops: (() => void)[] = [];
    let gone = false;
    const keep = (promise: Promise<() => void>) => void promise.then((stop) => (gone ? stop() : stops.push(stop)));
    keep(app.core.events.on("database:changed", ({ databaseId }) => databaseId === props.databaseId && void reload()));
    keep(app.core.events.on("tree:changed", ({ nodeIds }) => (nodeIds.includes(props.rowId) || nodeIds.includes(props.databaseId)) && void reload()));
    onCleanup(() => {
      gone = true;
      stops.forEach((stop) => stop());
    });
  });

  const optionsProperty = () => {
    const current = dialog();
    return current?.kind === "options" ? properties().find((prop) => prop.id === current.propertyId) : undefined;
  };

  return (
    <DatabaseProvider value={controller}>
      <DatabaseActionsProvider
        value={{
          openSchemaDialog: setDialog,
          openRow: (rowId, title, inNewTab) => app.openRow(props.databaseId, rowId, title, inNewTab),
          trashRow: async () => undefined,
        }}
      >
        <Show when={properties().length > 0}>
          <section class="row-properties" aria-label={t("db.rowProperties")}>
            <For each={properties()}>
              {(prop) => (
                <PropertyField
                  extra={
                    prop.type === "date" ? (
                      <IconButton
                        icon="bell"
                        size={14}
                        label={t("reminder.onProperty", { name: prop.name })}
                        onClick={() => app.ui.openDialog({ kind: "reminder", pageId: props.rowId, databaseId: props.databaseId, propertyId: prop.id })}
                      />
                    ) : undefined
                  }
                  property={prop}
                  value={row()?.values[prop.id]}
                  titles={titles()}
                  onChange={(value) => {
                    setRow((current) => {
                      if (!current) return current;
                      const values = { ...current.values };
                      if (value === null) delete values[prop.id];
                      else values[prop.id] = value;
                      return { ...current, values };
                    });
                    void controller.setCell(props.rowId, prop.id, value).then(reload);
                  }}
                />
              )}
            </For>
          </section>
        </Show>
        <Show when={optionsProperty()}>{(prop) => <OptionsDialog property={prop()} onClose={() => setDialog(null)} />}</Show>
      </DatabaseActionsProvider>
    </DatabaseProvider>
  );
}
