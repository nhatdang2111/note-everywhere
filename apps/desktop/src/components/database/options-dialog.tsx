import { For, Show, createSignal, type JSX } from "solid-js";
import type { DbProperty, OptionColor } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { OPTION_COLORS } from "../../lib/db/operators.ts";
import { Button, IconButton } from "../common/button.tsx";
import { Dialog } from "../common/dialog.tsx";
import { useDatabase } from "./database-context.tsx";
import { OptionChip } from "./option-chip.tsx";

/** Add, rename, recolour, reorder and delete the options of a select or multi-select property. */
export function OptionsDialog(props: { property: DbProperty; onClose(): void }): JSX.Element {
  const db = useDatabase();
  const [name, setName] = createSignal("");
  const add = async () => {
    const value = name().trim();
    if (!value) return;
    setName("");
    await db.addOption(props.property.id, value);
  };
  return (
    <Dialog title={t("db.optionsTitle", { name: props.property.name })} onClose={props.onClose} footer={<Button variant="primary" onClick={props.onClose}>{t("common.done")}</Button>}>
      <ul class="options-list">
        <For each={props.property.options}>
          {(option, at) => (
            <li class="options-row">
              <OptionChip name={option.name} color={option.color} />
              <input
                aria-label={t("db.optionName", { name: option.name })}
                value={option.name}
                maxLength={80}
                onKeyDown={(event) => event.key === "Enter" && event.currentTarget.blur()}
                onChange={(event) => event.currentTarget.value.trim() && void db.renameOption(props.property.id, option.id, event.currentTarget.value)}
              />
              <select aria-label={t("db.optionColor", { name: option.name })} value={option.color} onChange={(event) => void db.colorOption(props.property.id, option.id, event.currentTarget.value as OptionColor)}>
                <For each={OPTION_COLORS}>{(color) => <option value={color} selected={color === option.color}>{t(`db.color.${color}`)}</option>}</For>
              </select>
              <IconButton icon="arrow-up" size={14} label={t("db.optionUp", { name: option.name })} disabled={at() === 0} onClick={() => void db.moveOption(props.property.id, option.id, at() - 1)} />
              <IconButton icon="arrow-down" size={14} label={t("db.optionDown", { name: option.name })} disabled={at() === props.property.options.length - 1} onClick={() => void db.moveOption(props.property.id, option.id, at() + 1)} />
              <IconButton icon="trash" size={14} label={t("db.optionDelete", { name: option.name })} onClick={() => void db.deleteOption(props.property.id, option.id)} />
            </li>
          )}
        </For>
      </ul>
      <Show when={props.property.options.length === 0}>
        <p class="popover-empty">{t("db.noOptions")}</p>
      </Show>
      <form class="options-add" onSubmit={(event) => (event.preventDefault(), void add())}>
        <input aria-label={t("db.newOptionName")} placeholder={t("db.newOptionName")} value={name()} onInput={(event) => setName(event.currentTarget.value)} maxLength={80} />
        <Button type="submit" icon="plus" disabled={!name().trim()}>{t("db.addOption")}</Button>
      </form>
    </Dialog>
  );
}
