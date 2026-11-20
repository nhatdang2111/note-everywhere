import { For, Show, createResource, type JSX } from "solid-js";
import type { CellValue, DbProperty, TemplateSummary } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button, IconButton } from "../common/button.tsx";
import { Dialog } from "../common/dialog.tsx";
import { useDatabase } from "./database-context.tsx";
import { PropertyField } from "./property-field.tsx";

/** Row templates of this database: rename, delete, and choose the values a new row starts with. */
export function RowTemplatesDialog(props: { onClose(): void }): JSX.Element {
  const app = useApp();
  const db = useDatabase();
  const [templates, { refetch }] = createResource(() => app.core.api.templateList({ databaseId: db.id }));
  const editable = (): DbProperty[] => (db.schema()?.properties ?? []).filter((prop) => !prop.relation?.backlinkOf);

  const act = async (run: () => Promise<unknown>) => {
    await app.guard(run);
    await refetch();
  };
  const setDefault = (template: TemplateSummary, propertyId: string, value: CellValue | null) => {
    const defaults = { ...template.defaults };
    if (value === null) delete defaults[propertyId];
    else defaults[propertyId] = value;
    return act(() => app.core.api.templateSetDefaults({ id: template.id, defaults }));
  };

  return (
    <Dialog title={t("db.templatesTitle")} wide onClose={props.onClose} footer={<Button variant="primary" onClick={props.onClose}>{t("common.done")}</Button>}>
      <p class="view-intro">{t("db.templatesIntro")}</p>
      <Show when={(templates() ?? []).length === 0 && !templates.loading}>
        <p class="popover-empty">{t("db.templatesEmpty")}</p>
      </Show>
      <ul class="template-list">
        <For each={templates() ?? []}>
          {(template) => (
            <li class="template-item">
              <div class="template-head">
                <input
                  aria-label={t("template.name")}
                  value={template.name}
                  maxLength={80}
                  onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
                  onChange={(event) => event.currentTarget.value.trim() && void act(() => app.core.api.templateRename({ id: template.id, name: event.currentTarget.value }))}
                />
                <IconButton icon="trash" label={t("template.delete", { name: template.name })} onClick={() => void act(() => app.core.api.templateDelete({ id: template.id }))} />
              </div>
              <fieldset class="template-defaults">
                <legend>{t("db.templateDefaults")}</legend>
                <For each={editable()}>
                  {(prop) => <PropertyField property={prop} value={template.defaults[prop.id]} titles={db.titles()} onChange={(value) => void setDefault(template, prop.id, value)} />}
                </For>
                <Show when={editable().length === 0}>
                  <p class="popover-empty">{t("db.noProperties")}</p>
                </Show>
              </fieldset>
            </li>
          )}
        </For>
      </ul>
      <Button icon="plus" onClick={() => void act(() => app.core.api.templateCreateForDatabase({ databaseId: db.id, name: t("db.newTemplateName") }))}>
        {t("db.newTemplate")}
      </Button>
    </Dialog>
  );
}
