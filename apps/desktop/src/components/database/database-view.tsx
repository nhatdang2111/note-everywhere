import { Match, Show, Switch, createSignal, type JSX } from "solid-js";
import type { NodeId } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { createDatabaseController } from "../../state/database-store.ts";
import { Button } from "../common/button.tsx";
import { ErrorBanner, Spinner } from "../common/states.tsx";
import { BoardView } from "./board-view.tsx";
import { CalendarView } from "./calendar-view.tsx";
import { DatabaseActionsProvider, DatabaseProvider, type SchemaDialog } from "./database-context.tsx";
import { ListView } from "./list-view.tsx";
import { OptionsDialog } from "./options-dialog.tsx";
import { RowTemplatesDialog } from "./row-templates-dialog.tsx";
import { AddPropertyDialog, ChangeTypeDialog, DeletePropertyDialog, RenamePropertyDialog } from "./schema-dialogs.tsx";
import { TableView } from "./table-view.tsx";
import { ViewTabs } from "./view-tabs.tsx";
import { ViewToolbar } from "./view-toolbar.tsx";

/** A database in the main area: its title, view tabs, per-view controls and the rows in the active view. */
export function DatabaseView(props: { databaseId: NodeId }): JSX.Element {
  const app = useApp();
  const id = props.databaseId;
  const db = createDatabaseController(app, id);
  const [dialog, setDialog] = createSignal<SchemaDialog | null>(null);
  const [draft, setDraft] = createSignal<string | null>(null);

  const commitTitle = (value: string) => {
    const title = value.trim();
    setDraft(null);
    if (title !== app.tree.title(id)) void app.guard(() => app.tree.rename(id, title));
  };

  const propertyOf = () => {
    const current = dialog();
    if (!current || !("propertyId" in current)) return undefined;
    return db.schema()?.properties.find((prop) => prop.id === current.propertyId);
  };
  const close = () => setDialog(null);

  return (
    <DatabaseProvider value={db}>
      <DatabaseActionsProvider
        value={{
          openSchemaDialog: setDialog,
          openRow: (rowId, title, inNewTab) => app.openRow(id, rowId, title, inNewTab),
          async trashRow(rowId) {
            await app.guard(() => app.core.api.treeTrash({ id: rowId }));
            await db.refresh();
            app.toasts.push("info", t("db.rowTrashed"));
          },
        }}
      >
        <section class="database" aria-label={app.tree.title(id) || t("tree.untitled")}>
          <header class="database-header">
            <input
              class="page-title database-title"
              value={draft() ?? app.tree.title(id)}
              placeholder={t("db.untitled")}
              aria-label={t("db.titleLabel")}
              maxLength={300}
              onInput={(event) => setDraft(event.currentTarget.value)}
              onBlur={(event) => draft() !== null && commitTitle(event.currentTarget.value)}
              onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
            />
          </header>
          <ErrorBanner error={db.error()}>
            <Button onClick={() => void db.refresh()}>{t("common.retry")}</Button>
          </ErrorBanner>
          <Show when={db.schema()} fallback={<Show when={!db.error()}><Spinner /></Show>}>
            <ViewTabs />
            <ViewToolbar />
            <div class="database-body">
              <Switch>
                <Match when={db.view()?.kind === "board"}><BoardView /></Match>
                <Match when={db.view()?.kind === "list"}><ListView /></Match>
                <Match when={db.view()?.kind === "calendar"}><CalendarView /></Match>
                <Match when={true}><TableView /></Match>
              </Switch>
            </div>
          </Show>
          <Show when={dialog()?.kind === "add-property"}><AddPropertyDialog onClose={close} /></Show>
          <Show when={dialog()?.kind === "row-templates"}><RowTemplatesDialog onClose={close} /></Show>
          <Show when={dialog()?.kind === "rename-property" && propertyOf()}>{(prop) => <RenamePropertyDialog property={prop()} onClose={close} />}</Show>
          <Show when={dialog()?.kind === "delete-property" && propertyOf()}>{(prop) => <DeletePropertyDialog property={prop()} onClose={close} />}</Show>
          <Show when={dialog()?.kind === "change-type" && propertyOf()}>{(prop) => <ChangeTypeDialog property={prop()} onClose={close} />}</Show>
          <Show when={dialog()?.kind === "options" && propertyOf()}>{(prop) => <OptionsDialog property={prop()} onClose={close} />}</Show>
        </section>
      </DatabaseActionsProvider>
    </DatabaseProvider>
  );
}
