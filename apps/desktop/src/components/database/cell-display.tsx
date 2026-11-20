import { For, Match, Show, Switch, type JSX } from "solid-js";
import type { CellValue, DbProperty, NodeId } from "../../api/contract.ts";
import { isSafeHref } from "../../editor/index.ts";
import { formatDateValue, formatNumber } from "../../lib/format.ts";
import { useApp } from "../../state/app.tsx";
import { Icon } from "../common/icons.tsx";
import { OptionChip } from "./option-chip.tsx";

export type PropertyRef = DbProperty | "title";

export interface CellDisplayProps {
  property: PropertyRef;
  value: CellValue | undefined;
  /** The row's title, for the title property. */
  title?: string;
  /** Titles of the rows that relation values point at. */
  titles: Record<NodeId, string>;
}

const openLink = (event: MouseEvent, href: string) => {
  event.preventDefault();
  event.stopPropagation();
  if (isSafeHref(href)) window.open(href, "_blank", "noopener,noreferrer");
};

/** Read-only rendering of one cell value. */
export function CellDisplay(props: CellDisplayProps): JSX.Element {
  const app = useApp();
  const prop = () => (props.property === "title" ? null : props.property);
  const optionOf = (id: string) => prop()?.options.find((option) => option.id === id);
  const value = () => props.value;
  return (
    <Switch>
      <Match when={props.property === "title"}>
        <span class="cell-text">{props.title}</span>
      </Match>
      <Match when={value()?.type === "text" && value()}>{(text) => <span class="cell-text">{(text() as { value: string }).value}</span>}</Match>
      <Match when={value()?.type === "number" && value()}>{(number) => <span class="cell-number">{formatNumber((number() as { value: number }).value)}</span>}</Match>
      <Match when={value()?.type === "select" && value()}>
        {(select) => (
          <Show when={optionOf((select() as { value: string }).value)}>{(option) => <OptionChip name={option().name} color={option().color} />}</Show>
        )}
      </Match>
      <Match when={value()?.type === "multi_select" && value()}>
        {(multi) => (
          <span class="cell-chips">
            <For each={(multi() as { value: string[] }).value.flatMap((id) => optionOf(id) ?? [])}>{(option) => <OptionChip name={option.name} color={option.color} />}</For>
          </span>
        )}
      </Match>
      <Match when={value()?.type === "date" && value()}>{(date) => <span class="cell-date">{formatDateValue((date() as { value: { start: string; end?: string } }).value)}</span>}</Match>
      <Match when={prop()?.type === "checkbox"}>
        <span class="cell-check" data-checked={value()?.type === "checkbox" && value()?.value === true ? "true" : "false"} aria-hidden="true">
          <Show when={value()?.type === "checkbox" && value()?.value === true}>
            <Icon name="check" size={12} />
          </Show>
        </span>
      </Match>
      <Match when={value()?.type === "url" && value()}>
        {(url) => (
          <a class="cell-link" href={(url() as { value: string }).value} onClick={(event) => openLink(event, (url() as { value: string }).value)}>
            {(url() as { value: string }).value}
          </a>
        )}
      </Match>
      <Match when={value()?.type === "relation" && value()}>
        {(relation) => (
          <span class="cell-chips">
            <For each={(relation() as { value: string[] }).value}>
              {(id) => (
                <button
                  type="button"
                  class="cell-relation"
                  tabindex="-1"
                  onClick={(event) => {
                    event.stopPropagation();
                    const database = prop()?.relation?.database;
                    if (database) app.openRow(database, id, props.titles[id] ?? "");
                  }}
                >
                  <Icon name="page" size={12} />
                  {props.titles[id] ?? ""}
                </button>
              )}
            </For>
          </span>
        )}
      </Match>
    </Switch>
  );
}
