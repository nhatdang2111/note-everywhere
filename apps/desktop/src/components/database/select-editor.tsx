import { For, Show, createMemo, createSignal, createUniqueId, type JSX } from "solid-js";
import type { CellValue, DbProperty } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { fold } from "../../lib/text.ts";
import { Icon } from "../common/icons.tsx";
import { Popover } from "../common/popover.tsx";
import { useDatabase } from "./database-context.tsx";
import { OptionChip } from "./option-chip.tsx";

export interface SelectEditorProps {
  anchor: HTMLElement;
  property: DbProperty;
  value: CellValue | undefined;
  onChange(value: CellValue | null): void;
  onClose(): void;
  /** Open the dialog that renames, recolours and reorders the options. */
  onManageOptions(): void;
}

/** Pick one option (select) or tick several (multi-select), creating options as you type. */
export function SelectEditor(props: SelectEditorProps): JSX.Element {
  const db = useDatabase();
  const listId = createUniqueId();
  const multi = () => props.property.type === "multi_select";
  const [query, setQuery] = createSignal("");
  const [index, setIndex] = createSignal(0);

  const selected = createMemo<string[]>(() => {
    const value = props.value;
    if (value?.type === "select") return [value.value];
    if (value?.type === "multi_select") return value.value;
    return [];
  });

  const shown = createMemo(() => {
    const wanted = fold(query().trim());
    return props.property.options.filter((option) => fold(option.name).includes(wanted));
  });
  const typed = () => query().trim();
  const canCreate = () => typed() !== "" && !props.property.options.some((option) => fold(option.name) === fold(typed()));
  const rowCount = () => shown().length + (canCreate() ? 1 : 0);

  const commit = (ids: string[]) => {
    if (multi()) props.onChange(ids.length > 0 ? { type: "multi_select", value: ids } : null);
    else props.onChange(ids[0] ? { type: "select", value: ids[0] } : null);
  };

  const choose = (optionId: string) => {
    if (multi()) {
      commit(selected().includes(optionId) ? selected().filter((id) => id !== optionId) : [...selected(), optionId]);
    } else {
      commit(selected()[0] === optionId ? [] : [optionId]);
      props.onClose();
    }
  };

  const create = async () => {
    const name = typed();
    const created = await db.addOption(props.property.id, name);
    setQuery("");
    if (!created) return;
    if (multi()) commit([...selected(), created]);
    else {
      commit([created]);
      props.onClose();
    }
  };

  const activate = () => {
    const option = shown()[index()];
    if (option) choose(option.id);
    else if (canCreate()) void create();
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (event.key === "ArrowDown") (event.preventDefault(), setIndex((index() + 1) % Math.max(rowCount(), 1)));
    else if (event.key === "ArrowUp") (event.preventDefault(), setIndex((index() + Math.max(rowCount(), 1) - 1) % Math.max(rowCount(), 1)));
    else if (event.key === "Enter") (event.preventDefault(), activate());
  };

  return (
    <Popover anchor={props.anchor} label={t(multi() ? "db.editMulti" : "db.editSelect")} onClose={props.onClose} class="popover-select">
      <input
        class="popover-search"
        data-autofocus
        role="combobox"
        aria-expanded="true"
        aria-controls={listId}
        aria-activedescendant={`${listId}-${index()}`}
        placeholder={t("db.searchOptions")}
        value={query()}
        onInput={(event) => (setQuery(event.currentTarget.value), setIndex(0))}
        onKeyDown={onKeyDown}
      />
      <ul id={listId} class="popover-list" role="listbox" aria-multiselectable={multi()}>
        <For each={shown()}>
          {(option, at) => (
            <li
              id={`${listId}-${at()}`}
              role="option"
              class="popover-row"
              aria-selected={selected().includes(option.id)}
              data-active={index() === at() ? "true" : undefined}
              onMouseEnter={() => setIndex(at())}
              onMouseDown={(event) => event.preventDefault()}
              onClick={() => choose(option.id)}
            >
              <OptionChip name={option.name} color={option.color} />
              <Show when={selected().includes(option.id)}>
                <Icon name="check" size={14} />
              </Show>
            </li>
          )}
        </For>
        <Show when={canCreate()}>
          <li
            id={`${listId}-${shown().length}`}
            role="option"
            class="popover-row"
            aria-selected="false"
            data-active={index() === shown().length ? "true" : undefined}
            onMouseDown={(event) => event.preventDefault()}
            onClick={() => void create()}
          >
            <Icon name="plus" size={14} />
            <span>{t("db.createOption", { name: typed() })}</span>
          </li>
        </Show>
      </ul>
      <Show when={props.property.options.length === 0 && !canCreate()}>
        <p class="popover-empty">{t("db.noOptions")}</p>
      </Show>
      <div class="popover-footer">
        <button type="button" class="link-button" onClick={props.onManageOptions}>
          {t("db.manageOptions")}
        </button>
      </div>
    </Popover>
  );
}
