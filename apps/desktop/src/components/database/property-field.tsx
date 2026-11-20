import { createSignal, type JSX } from "solid-js";
import type { CellValue, DbProperty, NodeId } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { useDatabaseActionsOptional } from "./database-context.tsx";
import { EditableCell, isTextLike, startingText } from "./editable-cell.tsx";

export interface PropertyFieldProps {
  property: DbProperty;
  value: CellValue | undefined;
  titles: Record<NodeId, string>;
  onChange(value: CellValue | null): void;
  /** Open the dialog that edits select options; omitted where there is no database view around. */
  onManageOptions?(): void;
  /** Extra controls after the value, such as a reminder button. */
  extra?: JSX.Element;
}

/** One labelled property value outside a table: the same editors as a cell, opened by clicking or pressing Enter. */
export function PropertyField(props: PropertyFieldProps): JSX.Element {
  let box!: HTMLDivElement;
  const actions = useDatabaseActionsOptional();
  const [editing, setEditing] = createSignal(false);
  const [text, setText] = createSignal("");
  const checkbox = () => props.property.type === "checkbox";
  const checked = () => props.value?.type === "checkbox" && props.value.value;

  const start = () => {
    if (checkbox()) return;
    setText(isTextLike(props.property) ? startingText(props.property, props.value) : "");
    setEditing(true);
  };

  return (
    <div class="property-field">
      <span class="property-label" id={`pf-${props.property.id}`}>{props.property.name}</span>
      <div
        ref={box}
        class="property-value"
        role={checkbox() ? undefined : "button"}
        tabindex={checkbox() ? -1 : 0}
        aria-labelledby={`pf-${props.property.id}`}
        aria-haspopup={isTextLike(props.property) || checkbox() ? undefined : "dialog"}
        data-editing={editing() ? "true" : undefined}
        onClick={() => !editing() && start()}
        onKeyDown={(event) => {
          if (editing() || (event.key !== "Enter" && event.key !== " ")) return;
          event.preventDefault();
          start();
        }}
      >
        {checkbox() ? (
          <input type="checkbox" class="cell-checkbox" aria-label={props.property.name} checked={checked()} onChange={(event) => props.onChange({ type: "checkbox", value: event.currentTarget.checked })} />
        ) : (
          <EditableCell
            property={props.property}
            value={props.value}
            titles={props.titles}
            editing={editing()}
            text={text()}
            onText={setText}
            onChange={props.onChange}
            onExit={() => {
              setEditing(false);
              queueMicrotask(() => box.focus());
            }}
            anchor={() => box}
            onManageOptions={() => (props.onManageOptions ?? (() => actions?.openSchemaDialog({ kind: "options", propertyId: props.property.id })))()}
          />
        )}
        {!editing() && !checkbox() && !props.value && <span class="property-empty">{t("db.emptyValue")}</span>}
      </div>
      {props.extra}
    </div>
  );
}
