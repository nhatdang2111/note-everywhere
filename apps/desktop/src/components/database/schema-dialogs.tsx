import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import type { DbProperty, PropertyType } from "../../api/contract.ts";
import { t, type MessageKey } from "../../i18n/index.ts";
import { conversionKind, optionNamesFor, readCell, cellToText } from "../../lib/db/cell-convert.ts";
import { PROPERTY_TYPES, nextOptionColor } from "../../lib/db/operators.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";
import { Dialog } from "../common/dialog.tsx";
import { Select, TextField } from "../common/form.tsx";
import { useDatabase } from "./database-context.tsx";

const typeOptions = (exclude?: PropertyType) =>
  PROPERTY_TYPES.filter((type) => type !== exclude).map((type) => ({ value: type, label: t(`db.type.${type}`) }));

function useDatabaseChoices() {
  const app = useApp();
  return () => app.tree.nodes().filter((node) => node.kind === "database").map((node) => ({ value: node.id, label: node.title || t("tree.untitled") }));
}

export function AddPropertyDialog(props: { onClose(): void }): JSX.Element {
  const db = useDatabase();
  const databases = useDatabaseChoices();
  const [name, setName] = createSignal("");
  const [type, setType] = createSignal<PropertyType>("text");
  const [target, setTarget] = createSignal(db.id);
  const [reverse, setReverse] = createSignal("");
  const [busy, setBusy] = createSignal(false);

  const submit = async () => {
    if (!name().trim()) return;
    setBusy(true);
    const id = type() === "relation" ? await db.addRelation(name(), target(), reverse().trim() || undefined) : await db.addProperty(name(), type());
    setBusy(false);
    if (id) props.onClose();
  };

  return (
    <Dialog
      title={t("db.addProperty")}
      onClose={props.onClose}
      dismissOnBackdrop={false}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" busy={busy()} disabled={!name().trim()} onClick={() => void submit()}>{t("db.addProperty")}</Button>
        </>
      }
    >
      <form onSubmit={(event) => (event.preventDefault(), void submit())}>
        <TextField label={t("db.propertyName")} value={name()} onInput={setName} autofocus maxLength={80} />
        <Select<PropertyType> label={t("db.propertyType")} value={type()} options={typeOptions()} onChange={setType} />
        <Show when={type() === "relation"}>
          <Select label={t("db.relationTarget")} value={target()} options={databases()} onChange={setTarget} />
          <TextField label={t("db.relationReverse")} value={reverse()} onInput={setReverse} hint={t("db.relationReverseHint")} />
        </Show>
      </form>
    </Dialog>
  );
}

export function RenamePropertyDialog(props: { property: DbProperty; onClose(): void }): JSX.Element {
  const db = useDatabase();
  const [name, setName] = createSignal(props.property.name);
  const submit = async () => {
    if (!name().trim()) return;
    if (name().trim() === props.property.name || (await db.renameProperty(props.property.id, name()))) props.onClose();
  };
  return (
    <Dialog
      title={t("db.renameProperty")}
      onClose={props.onClose}
      dismissOnBackdrop={false}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" disabled={!name().trim()} onClick={() => void submit()}>{t("common.save")}</Button>
        </>
      }
    >
      <form onSubmit={(event) => (event.preventDefault(), void submit())}>
        <TextField label={t("db.propertyName")} value={name()} onInput={setName} autofocus maxLength={80} />
      </form>
    </Dialog>
  );
}

export function DeletePropertyDialog(props: { property: DbProperty; onClose(): void }): JSX.Element {
  const db = useDatabase();
  return (
    <ConfirmDialog
      title={t("db.deletePropertyTitle", { name: props.property.name })}
      message={t("db.deletePropertyBody")}
      confirmLabel={t("common.delete")}
      danger
      onCancel={props.onClose}
      onConfirm={() => void db.deleteProperty(props.property.id).then(props.onClose)}
    />
  );
}

/** What each kind of source value becomes, for the sentence under the type picker. */
const SOURCE_NOTE: Record<PropertyType, MessageKey> = {
  text: "db.conv.text",
  url: "db.conv.text",
  number: "db.conv.number",
  select: "db.conv.select",
  multi_select: "db.conv.multi_select",
  date: "db.conv.date",
  checkbox: "db.conv.checkbox",
  relation: "db.conv.relation",
};

/** Change a property's type after showing what the conversion does to real values. */
export function ChangeTypeDialog(props: { property: DbProperty; onClose(): void }): JSX.Element {
  const db = useDatabase();
  const databases = useDatabaseChoices();
  const [type, setType] = createSignal<PropertyType>(props.property.type === "text" ? "number" : "text");
  const [target, setTarget] = createSignal(db.id);
  const [busy, setBusy] = createSignal(false);
  const kind = () => conversionKind(props.property.type, type());

  const preview = createMemo(() => {
    const samples = Array.from({ length: 5 }, (_, at) => db.rowAt(at)).flatMap((row) => (row ? [row] : []));
    const from = props.property;
    let options = from.options;
    if ((type() === "select" || type() === "multi_select") && (from.type === "text" || from.type === "url")) {
      const names = samples.flatMap((row) => {
        const value = row.values[from.id];
        return value && (value.type === "text" || value.type === "url") ? optionNamesFor(value.value, type() as "select" | "multi_select") : [];
      });
      options = [...from.options, ...[...new Set(names)].map((name, at) => ({ id: `new-${at}`, name, color: nextOptionColor(at) }))];
    }
    const converted: DbProperty = { ...from, type: type(), options };
    return samples.map((row) => {
      const value = row.values[from.id];
      return { id: row.id, title: row.title, before: cellToText(from, value, db.titles()), after: cellToText(converted, readCell(converted, value), db.titles()) };
    });
  });

  const submit = async () => {
    setBusy(true);
    const ok = await db.changeType(props.property.id, type(), type() === "relation" ? target() : undefined);
    setBusy(false);
    if (ok) props.onClose();
  };

  return (
    <Dialog
      title={t("db.changeTypeTitle", { name: props.property.name })}
      onClose={props.onClose}
      wide
      dismissOnBackdrop={false}
      footer={
        <>
          <Button data-autofocus onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" busy={busy()} disabled={kind() === "same"} onClick={() => void submit()}>{t("db.changeTypeConfirm")}</Button>
        </>
      }
    >
      <p>{t("db.changeTypeFrom", { type: t(`db.type.${props.property.type}`) })}</p>
      <Select<PropertyType> label={t("db.propertyType")} value={type()} options={typeOptions()} onChange={setType} />
      <Show when={type() === "relation"}>
        <Select label={t("db.relationTarget")} value={target()} options={databases()} onChange={setTarget} />
      </Show>
      <div class="conversion-note" role="status" data-kind={kind()}>
        <Show when={kind() === "same"}>{t("db.conv.same")}</Show>
        <Show when={kind() === "keeps"}>
          <p>{t("db.conv.keeps")}</p>
          <p>{t(SOURCE_NOTE[props.property.type])}</p>
        </Show>
        <Show when={kind() === "blank"}>{t("db.conv.blank")}</Show>
      </div>
      <Show when={preview().length > 0 && kind() !== "same"}>
        <table class="conversion-preview">
          <caption>{t("db.conv.preview")}</caption>
          <thead>
            <tr>
              <th scope="col">{t("db.title")}</th>
              <th scope="col">{t("db.conv.before")}</th>
              <th scope="col">{t("db.conv.after")}</th>
            </tr>
          </thead>
          <tbody>
            <For each={preview()}>
              {(row) => (
                <tr>
                  <td>{row.title || t("tree.untitled")}</td>
                  <td>{row.before || "—"}</td>
                  <td>{row.after || t("db.conv.empty")}</td>
                </tr>
              )}
            </For>
          </tbody>
        </table>
      </Show>
    </Dialog>
  );
}
