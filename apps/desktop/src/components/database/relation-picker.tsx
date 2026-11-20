import { For, Show, createResource, createSignal, createUniqueId, type JSX } from "solid-js";
import type { CellValue, DbProperty, NodeId } from "../../api/contract.ts";
import { toApiError } from "../../api/errors.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Icon } from "../common/icons.tsx";
import { ErrorBanner, Spinner } from "../common/states.tsx";
import { Popover } from "../common/popover.tsx";
import { useDatabase } from "./database-context.tsx";

export interface RelationPickerProps {
  anchor: HTMLElement;
  property: DbProperty;
  value: CellValue | undefined;
  /** Titles already known for the linked rows. */
  titles: Record<NodeId, string>;
  onChange(value: CellValue | null): void;
  onClose(): void;
}

const RESULT_LIMIT = 30;

/** Search the rows of the related database and tick the ones this row links to. */
export function RelationPicker(props: RelationPickerProps): JSX.Element {
  const app = useApp();
  const db = useDatabase();
  const listId = createUniqueId();
  const [query, setQuery] = createSignal("");
  const [index, setIndex] = createSignal(0);
  const [known, setKnown] = createSignal<Record<NodeId, string>>({});
  const target = () => props.property.relation?.database;
  const backlink = () => Boolean(props.property.relation?.backlinkOf);
  const linked = (): NodeId[] => (props.value?.type === "relation" ? props.value.value : []);
  const titleOf = (id: NodeId) => known()[id] ?? props.titles[id] ?? db.titles()[id] ?? "";

  const [results] = createResource(
    () => ({ database: target(), text: query().trim() }),
    async ({ database, text }) => {
      if (!database) return [];
      const result = await app.core.api.dbQuery({
        id: database,
        spec: { filter: text ? { type: "condition", property: "title", operator: "contains", value: text } : null, sorts: [{ property: "title", descending: false }], limit: RESULT_LIMIT },
      });
      setKnown((previous) => ({ ...previous, ...Object.fromEntries(result.rows.map((row) => [row.id, row.title])) }));
      return result.rows;
    },
  );

  const toggle = (id: NodeId) => {
    const next = linked().includes(id) ? linked().filter((candidate) => candidate !== id) : [...linked(), id];
    props.onChange(next.length > 0 ? { type: "relation", value: next } : null);
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const count = results()?.length ?? 0;
    if (event.key === "ArrowDown") (event.preventDefault(), setIndex(count ? (index() + 1) % count : 0));
    else if (event.key === "ArrowUp") (event.preventDefault(), setIndex(count ? (index() + count - 1) % count : 0));
    else if (event.key === "Enter") {
      event.preventDefault();
      const row = results()?.[index()];
      if (row) toggle(row.id);
    }
  };

  return (
    <Popover anchor={props.anchor} label={t("db.editRelation")} onClose={props.onClose} class="popover-select">
      <Show when={linked().length > 0}>
        <ul class="relation-selected" aria-label={t("db.relationLinked")}>
          <For each={linked()}>
            {(id) => (
              <li>
                <span class="cell-relation">
                  <Icon name="page" size={12} />
                  {titleOf(id)}
                </span>
                <button type="button" class="opt-remove" aria-label={t("db.relationUnlink", { title: titleOf(id) })} onClick={() => toggle(id)}>
                  ×
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <input
        class="popover-search"
        data-autofocus
        role="combobox"
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={`${listId}-${index()}`}
        placeholder={t("db.relationSearch")}
        value={query()}
        onInput={(event) => (setQuery(event.currentTarget.value), setIndex(0))}
        onKeyDown={onKeyDown}
      />
      <Show when={backlink()}>
        <p class="popover-empty">{t("db.relationBacklinkHint")}</p>
      </Show>
      <ErrorBanner error={results.error ? toApiError(results.error) : null} />
      <Show when={results.loading && !results()}>
        <Spinner />
      </Show>
      <ul id={listId} class="popover-list" role="listbox" aria-multiselectable="true">
        <For each={results() ?? []}>
          {(row, at) => (
            <li
              id={`${listId}-${at()}`}
              role="option"
              class="popover-row"
              aria-selected={linked().includes(row.id)}
              data-active={index() === at() ? "true" : undefined}
              onMouseEnter={() => setIndex(at())}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => toggle(row.id)}
            >
              <span class="popover-row-label">{row.title || t("tree.untitled")}</span>
              <Show when={linked().includes(row.id)}>
                <Icon name="check" size={14} />
              </Show>
            </li>
          )}
        </For>
      </ul>
      <Show when={results() && results()?.length === 0}>
        <p class="popover-empty">{t("db.relationNoRows")}</p>
      </Show>
    </Popover>
  );
}
