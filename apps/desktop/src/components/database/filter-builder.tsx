import { For, Show, createSignal, type JSX } from "solid-js";
import type { DatabaseSchema, DbProperty, FilterNode, FilterOperator } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import {
  MAX_GROUP_DEPTH,
  emptyGroup,
  findNode,
  fromFilter,
  newCondition,
  toFilter,
  typeOfProperty,
  updateNode,
  withProperty,
  type BuilderCondition,
  type BuilderGroup,
  type BuilderNode,
} from "../../lib/db/filter-model.ts";
import { OPERATORS, needsValue } from "../../lib/db/operators.ts";
import { Button, IconButton } from "../common/button.tsx";

export interface FilterBuilderProps {
  schema: DatabaseSchema;
  /** The saved filter the builder starts from; later changes come from the builder itself. */
  filter: FilterNode | null;
  /** Called with the filter to save whenever a complete change is made. */
  onChange(filter: FilterNode | null): void;
}

const filterable = (props: DbProperty[]) => props.filter((prop) => !prop.relation?.backlinkOf || prop.type === "relation");

/** Nested and/or groups of conditions with the operators that suit each property type. */
export function FilterBuilder(props: FilterBuilderProps): JSX.Element {
  const [root, setRoot] = createSignal<BuilderGroup>(fromFilter(props.filter));
  const properties = () => props.schema.properties;

  let saved = JSON.stringify(props.filter);
  const apply = (next: BuilderGroup) => {
    setRoot(next);
    const filter = toFilter(next, properties());
    // A half-written condition changes nothing to send.
    if (JSON.stringify(filter) === saved) return;
    saved = JSON.stringify(filter);
    props.onChange(filter);
  };
  const change = (id: string, edit: (node: BuilderNode) => BuilderNode | null) => apply(updateNode(root(), id, edit));
  const group = (id: string) => findNode(root(), id) as BuilderGroup | undefined;
  const condition = (id: string) => findNode(root(), id) as BuilderCondition | undefined;

  // Rows are drawn by id, so typing in a value keeps the input (and its focus) while the tree changes around it.
  function Condition(rowProps: { id: string }): JSX.Element {
    const node = () => condition(rowProps.id);
    const type = () => typeOfProperty(properties(), node()?.property ?? "title") ?? "text";
    const prop = () => properties().find((candidate) => candidate.id === node()?.property);
    const setValue = (value: string) => change(rowProps.id, (current) => (current.type === "condition" ? { ...current, value } : current));
    return (
      <Show when={node()}>
        {(current) => (
          <div class="filter-row">
            <select
              aria-label={t("db.filterProperty")}
              value={current().property}
              onChange={(event) => change(rowProps.id, (existing) => (existing.type === "condition" ? withProperty(existing, properties(), event.currentTarget.value) : existing))}
            >
              <option value="title" selected={current().property === "title"}>{t("db.title")}</option>
              <For each={filterable(properties())}>{(candidate) => <option value={candidate.id} selected={candidate.id === current().property}>{candidate.name}</option>}</For>
            </select>
            <select
              aria-label={t("db.filterOperator")}
              value={current().operator}
              onChange={(event) => change(rowProps.id, (existing) => (existing.type === "condition" ? { ...existing, operator: event.currentTarget.value as FilterOperator } : existing))}
            >
              <For each={OPERATORS[type()]}>{(operator) => <option value={operator} selected={operator === current().operator}>{t(`db.op.${operator}`)}</option>}</For>
            </select>
            <Show when={needsValue(current().operator)}>
              <Show
                when={type() === "select" || type() === "multi_select"}
                fallback={
                  <input
                    aria-label={t("db.filterValue")}
                    type={type() === "date" ? "date" : type() === "number" ? "number" : "text"}
                    value={current().value}
                    placeholder={type() === "relation" ? t("db.filterRowId") : undefined}
                    onInput={(event) => setValue(event.currentTarget.value)}
                  />
                }
              >
                <select aria-label={t("db.filterValue")} value={current().value} onChange={(event) => setValue(event.currentTarget.value)}>
                  <option value="" />
                  <For each={prop()?.options ?? []}>{(option) => <option value={option.id} selected={option.id === current().value}>{option.name}</option>}</For>
                </select>
              </Show>
            </Show>
            <IconButton icon="close" size={14} label={t("db.filterRemove")} onClick={() => change(rowProps.id, () => null)} />
          </div>
        )}
      </Show>
    );
  }

  function Group(groupProps: { id: string; depth: number }): JSX.Element {
    const node = () => group(groupProps.id);
    const childIds = () => (node()?.children ?? []).map((child) => `${child.type}:${child.id}`);
    return (
      <Show when={node()}>
        {(current) => (
          <div class="filter-group" role="group" aria-label={t("db.filterGroup", { depth: groupProps.depth })} data-depth={groupProps.depth}>
            <Show when={current().children.length > 1}>
              <label class="filter-logic">
                <span class="visually-hidden">{t("db.filterLogic")}</span>
                <select value={current().logic} onChange={(event) => change(groupProps.id, (existing) => (existing.type === "group" ? { ...existing, logic: event.currentTarget.value as "and" | "or" } : existing))}>
                  <option value="and" selected={current().logic === "and"}>{t("db.filterAnd")}</option>
                  <option value="or" selected={current().logic === "or"}>{t("db.filterOr")}</option>
                </select>
              </label>
            </Show>
            <For each={childIds()}>
              {(key) => {
                const [kind, id] = key.split(":") as ["group" | "condition", string];
                return kind === "group" ? (
                  <div class="filter-nested">
                    <Group id={id} depth={groupProps.depth + 1} />
                    <IconButton icon="trash" size={14} label={t("db.filterRemoveGroup")} onClick={() => change(id, () => null)} />
                  </div>
                ) : (
                  <Condition id={id} />
                );
              }}
            </For>
            <div class="filter-add">
              <Button variant="ghost" icon="plus" onClick={() => change(groupProps.id, (existing) => (existing.type === "group" ? { ...existing, children: [...existing.children, newCondition(properties(), "title")] } : existing))}>
                {t("db.filterAdd")}
              </Button>
              <Show when={groupProps.depth < MAX_GROUP_DEPTH}>
                <Button variant="ghost" icon="plus" onClick={() => change(groupProps.id, (existing) => (existing.type === "group" ? { ...existing, children: [...existing.children, { ...emptyGroup(), children: [newCondition(properties(), "title")] }] } : existing))}>
                  {t("db.filterAddGroup")}
                </Button>
              </Show>
            </div>
          </div>
        )}
      </Show>
    );
  }

  return (
    <div class="filter-builder">
      <Group id={root().id} depth={1} />
    </div>
  );
}
