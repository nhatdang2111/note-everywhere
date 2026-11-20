import { For, Show, createResource, createSignal, untrack, type JSX } from "solid-js";
import type { DatabaseSchema, ViewDef } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { countConditions, fromFilter } from "../../lib/db/filter-model.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Menu, type MenuEntry } from "../common/menu.tsx";
import { Popover } from "../common/popover.tsx";
import { useDatabase, useDatabaseActions } from "./database-context.tsx";
import { FilterBuilder } from "./filter-builder.tsx";
import { GroupEditor, PropertiesPicker, SortEditor } from "./view-settings.tsx";

type Panel = "filter" | "sort" | "group" | "properties" | "date" | "new";

/** The body of one toolbar popover. It reads the controller itself so editing the view does not rebuild it. */
function Panel(props: { panel: Panel }): JSX.Element {
  const db = useDatabase();
  const schema = () => db.schema() as DatabaseSchema;
  const view = () => db.view() as ViewDef;
  const save = (next: ViewDef) => void db.saveView(next);
  return (
    <>
      <Show when={props.panel === "filter"}>
        <FilterBuilder schema={schema()} filter={untrack(() => view().filter)} onChange={(filter) => save({ ...view(), filter })} />
      </Show>
      <Show when={props.panel === "sort"}>
        <SortEditor schema={schema()} view={view()} onSave={save} />
      </Show>
      <Show when={props.panel === "group"}>
        <GroupEditor schema={schema()} view={view()} onSave={save} />
      </Show>
      <Show when={props.panel === "properties"}>
        <PropertiesPicker schema={schema()} view={view()} onSave={save} />
      </Show>
      <Show when={props.panel === "date"}>
        <div class="field">
          <label for="calendar-date">{t("db.dateProperty")}</label>
          <select id="calendar-date" value={view().dateProperty ?? ""} onChange={(event) => save({ ...view(), dateProperty: event.currentTarget.value || null })}>
            <option value="" selected={!view().dateProperty}>{t("db.groupNone")}</option>
            <For each={schema().properties.filter((prop) => prop.type === "date")}>{(prop) => <option value={prop.id} selected={prop.id === view().dateProperty}>{prop.name}</option>}</For>
          </select>
        </div>
      </Show>
    </>
  );
}

/** Per-view controls: filter, sort, group, visible properties, and creating rows (optionally from a template). */
export function ViewToolbar(): JSX.Element {
  const app = useApp();
  const db = useDatabase();
  const actions = useDatabaseActions();
  const [open, setOpen] = createSignal<{ panel: Panel; anchor: HTMLElement } | null>(null);
  const [templates, { refetch }] = createResource(() => app.core.api.templateList({ databaseId: db.id }));
  const view = () => db.view();
  const schema = () => db.schema();
  const conditions = () => (view()?.filter ? countConditions(fromFilter(view()?.filter ?? null)) : 0);
  const toggle = (panel: Panel) => (event: MouseEvent) => {
    const anchor = event.currentTarget as HTMLElement;
    setOpen(open()?.panel === panel ? null : { panel, anchor });
  };

  const newEntries = (): MenuEntry[] => [
    ...(templates() ?? []).map((template) => ({
      kind: "item" as const,
      icon: "page" as const,
      label: template.name,
      onSelect: () => void db.createRowFromTemplate(template.id),
    })),
    ...((templates() ?? []).length > 0 ? [{ kind: "separator" as const }] : []),
    { kind: "item", icon: "settings", label: t("db.manageTemplates"), onSelect: () => actions.openSchemaDialog({ kind: "row-templates" }) },
  ];
  const [menuAt, setMenuAt] = createSignal<{ x: number; y: number } | null>(null);

  return (
    <div class="view-toolbar" role="toolbar" aria-label={t("db.toolbar")}>
      <Button variant="ghost" aria-expanded={open()?.panel === "filter"} onClick={toggle("filter")}>
        {t("db.filter")}
        <Show when={conditions() > 0}>
          <span class="badge" aria-label={t("db.activeCount", { count: conditions() })}>{conditions()}</span>
        </Show>
      </Button>
      <Button variant="ghost" aria-expanded={open()?.panel === "sort"} onClick={toggle("sort")}>
        {t("db.sort")}
        <Show when={(view()?.sorts.length ?? 0) > 0}>
          <span class="badge" aria-label={t("db.activeCount", { count: view()?.sorts.length ?? 0 })}>{view()?.sorts.length}</span>
        </Show>
      </Button>
      <Show when={view()?.kind === "board"}>
        <Button variant="ghost" aria-expanded={open()?.panel === "group"} onClick={toggle("group")}>{t("db.group")}</Button>
      </Show>
      <Show when={view()?.kind === "calendar"}>
        <Button variant="ghost" aria-expanded={open()?.panel === "date"} onClick={toggle("date")}>{t("db.dateProperty")}</Button>
      </Show>
      <Button variant="ghost" aria-expanded={open()?.panel === "properties"} onClick={toggle("properties")}>{t("db.properties")}</Button>
      <span class="toolbar-spacer" />
      <div class="split-button">
        <Button variant="primary" icon="plus" onClick={() => void db.createRow()}>{t("db.newRow")}</Button>
        <Button
          variant="primary"
          aria-label={t("db.newFromTemplate")}
          aria-haspopup="menu"
          onClick={(event) => {
            void refetch();
            const rect = event.currentTarget.getBoundingClientRect();
            setMenuAt({ x: rect.left - 160, y: rect.bottom + 4 });
          }}
        >
          ▾
        </Button>
      </div>
      <Show when={menuAt()}>{(at) => <Menu label={t("db.newFromTemplate")} at={at()} entries={newEntries()} onClose={() => setMenuAt(null)} />}</Show>
      <Show when={open()}>
        {(current) => (
          <Popover anchor={current().anchor} label={t(`db.panel.${current().panel}`)} onClose={() => setOpen(null)} class="popover-wide">
            <Show when={Boolean(schema() && view())}>
              <Panel panel={current().panel} />
            </Show>
          </Popover>
        )}
      </Show>
    </div>
  );
}
