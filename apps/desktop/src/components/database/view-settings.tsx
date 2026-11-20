import { For, Show, type JSX } from "solid-js";
import type { DatabaseSchema, ViewDef } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { GROUPABLE } from "../../lib/db/operators.ts";
import { orderedColumns, withColumnMoved, withColumnVisible } from "../../lib/db/view-model.ts";
import { Button, IconButton } from "../common/button.tsx";
import { Checkbox } from "../common/form.tsx";

interface Props {
  schema: DatabaseSchema;
  view: ViewDef;
  onSave(view: ViewDef): void;
}

/** Sort keys in priority order; each can be changed, reversed or removed. */
export function SortEditor(props: Props): JSX.Element {
  const sortable = () => [{ id: "title", name: t("db.title") }, ...props.schema.properties.map((prop) => ({ id: prop.id, name: prop.name }))];
  const set = (sorts: ViewDef["sorts"]) => props.onSave({ ...props.view, sorts });
  return (
    <div class="sort-editor">
      <Show when={props.view.sorts.length === 0}>
        <p class="popover-empty">{t("db.noSorts")}</p>
      </Show>
      <For each={props.view.sorts}>
        {(sort, at) => (
          <div class="filter-row">
            <select aria-label={t("db.sortProperty")} value={sort.property} onChange={(event) => set(props.view.sorts.map((current, i) => (i === at() ? { ...current, property: event.currentTarget.value } : current)))}>
              <For each={sortable()}>{(option) => <option value={option.id} selected={option.id === sort.property}>{option.name}</option>}</For>
            </select>
            <select aria-label={t("db.sortDirection")} value={sort.descending ? "desc" : "asc"} onChange={(event) => set(props.view.sorts.map((current, i) => (i === at() ? { ...current, descending: event.currentTarget.value === "desc" } : current)))}>
              <option value="asc" selected={!sort.descending}>{t("db.sortAscending")}</option>
              <option value="desc" selected={sort.descending}>{t("db.sortDescending")}</option>
            </select>
            <IconButton icon="arrow-up" size={14} label={t("db.sortEarlier")} disabled={at() === 0} onClick={() => set(swap(props.view.sorts, at(), at() - 1))} />
            <IconButton icon="arrow-down" size={14} label={t("db.sortLater")} disabled={at() === props.view.sorts.length - 1} onClick={() => set(swap(props.view.sorts, at(), at() + 1))} />
            <IconButton icon="close" size={14} label={t("db.sortRemove")} onClick={() => set(props.view.sorts.filter((_, i) => i !== at()))} />
          </div>
        )}
      </For>
      <Button
        variant="ghost"
        icon="plus"
        disabled={props.view.sorts.length >= sortable().length}
        onClick={() => {
          const used = new Set(props.view.sorts.map((sort) => sort.property));
          const next = sortable().find((option) => !used.has(option.id));
          if (next) set([...props.view.sorts, { property: next.id, descending: false }]);
        }}
      >
        {t("db.sortAdd")}
      </Button>
    </div>
  );
}

function swap<T>(list: T[], a: number, b: number): T[] {
  const next = [...list];
  [next[a], next[b]] = [next[b] as T, next[a] as T];
  return next;
}

/** The property a board groups by, and whether empty groups show. */
export function GroupEditor(props: Props): JSX.Element {
  const options = () => props.schema.properties.filter((prop) => GROUPABLE.includes(prop.type) && !prop.relation?.backlinkOf);
  return (
    <div class="group-editor">
      <div class="field">
        <label for="group-property">{t("db.groupBy")}</label>
        <select
          id="group-property"
          value={props.view.group?.property ?? ""}
          onChange={(event) => props.onSave({ ...props.view, group: event.currentTarget.value ? { property: event.currentTarget.value, hideEmpty: props.view.group?.hideEmpty ?? false } : null })}
        >
          <option value="" selected={!props.view.group}>{t("db.groupNone")}</option>
          <For each={options()}>{(prop) => <option value={prop.id} selected={prop.id === props.view.group?.property}>{prop.name}</option>}</For>
        </select>
      </div>
      <Show when={props.view.group}>
        {(group) => <Checkbox label={t("db.hideEmptyGroups")} checked={group().hideEmpty} onChange={(hideEmpty) => props.onSave({ ...props.view, group: { ...group(), hideEmpty } })} />}
      </Show>
    </div>
  );
}

/** Which properties the view shows, and in what order. */
export function PropertiesPicker(props: Props): JSX.Element {
  const columns = () => orderedColumns(props.schema, props.view);
  return (
    <ul class="properties-picker">
      <For each={columns()}>
        {(column, at) => (
          <li class="filter-row">
            <Checkbox label={column.property.name} checked={column.visible} onChange={(visible) => props.onSave(withColumnVisible(props.schema, props.view, column.property.id, visible))} />
            <IconButton icon="arrow-up" size={14} label={t("db.moveUpProperty", { name: column.property.name })} disabled={at() === 0} onClick={() => props.onSave(withColumnMoved(props.schema, props.view, column.property.id, at() - 1))} />
            <IconButton icon="arrow-down" size={14} label={t("db.moveDownProperty", { name: column.property.name })} disabled={at() === columns().length - 1} onClick={() => props.onSave(withColumnMoved(props.schema, props.view, column.property.id, at() + 1))} />
          </li>
        )}
      </For>
      <Show when={columns().length === 0}>
        <li class="popover-empty">{t("db.noProperties")}</li>
      </Show>
    </ul>
  );
}
