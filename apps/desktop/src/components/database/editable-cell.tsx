import { Match, Show, Switch, onMount, type JSX } from "solid-js";
import type { CellValue, NodeId } from "../../api/contract.ts";
import { cellToText } from "../../lib/db/cell-convert.ts";
import { t } from "../../i18n/index.ts";
import { CellDisplay, type PropertyRef } from "./cell-display.tsx";
import { DateEditor } from "./date-editor.tsx";
import { RelationPicker } from "./relation-picker.tsx";
import { SelectEditor } from "./select-editor.tsx";

/** How an edit ended, so a table can move on the way a spreadsheet would. */
export type EditExit = "enter" | "tab" | "shift-tab" | "escape" | "blur";

export interface EditableCellProps {
  property: PropertyRef;
  value: CellValue | undefined;
  title?: string;
  titles: Record<NodeId, string>;
  editing: boolean;
  /** The text being typed, for text-like properties; kept by the owner so it survives the cell being redrawn. */
  text?: string;
  onText?(text: string): void;
  /** Save a value without leaving the edit (popover editors save as you go). */
  onChange(value: CellValue | null): void;
  onExit(exit: EditExit): void;
  /** The element popovers hang from. */
  anchor: () => HTMLElement | undefined;
  onManageOptions(): void;
}

export const isTextLike = (property: PropertyRef): boolean => property === "title" || ["text", "number", "url"].includes(property.type);

/** Text a text-like cell starts editing with. */
export const startingText = (property: PropertyRef, value: CellValue | undefined, title?: string): string =>
  property === "title" ? (title ?? "") : cellToText(property, value);

/** The value typed text means for a property, `null` to clear, or `undefined` when it is not valid. */
export function valueFromText(property: PropertyRef, text: string): CellValue | null | undefined {
  if (property === "title") return { type: "text", value: text.trim() };
  if (property.type === "number") {
    const trimmed = text.trim();
    if (trimmed === "") return null;
    const parsed = Number(trimmed.replace(",", "."));
    return Number.isFinite(parsed) ? { type: "number", value: parsed } : undefined;
  }
  if (property.type === "url" || property.type === "text") return text.trim() === "" ? null : { type: property.type, value: text.trim() };
  return undefined;
}

function TextEditor(props: EditableCellProps): JSX.Element {
  let input!: HTMLInputElement;
  let finished = false;
  onMount(() => {
    input.focus();
    input.setSelectionRange(input.value.length, input.value.length);
  });
  const finish = (exit: EditExit) => {
    if (finished) return;
    if (exit !== "escape") {
      const value = valueFromText(props.property, input.value);
      if (value === undefined) {
        // An invalid number: stay and say so, unless the person is just leaving.
        if (exit !== "blur") return void input.setAttribute("aria-invalid", "true");
        finished = true;
        return props.onExit("escape");
      }
      props.onChange(value);
    }
    finished = true;
    props.onExit(exit);
  };
  return (
    <input
      ref={input}
      class="cell-input"
      type="text"
      inputMode={props.property !== "title" && props.property.type === "number" ? "decimal" : undefined}
      aria-label={props.property === "title" ? t("db.title") : props.property.name}
      value={props.text ?? ""}
      onInput={(event) => {
        event.currentTarget.removeAttribute("aria-invalid");
        props.onText?.(event.currentTarget.value);
      }}
      onKeyDown={(event) => {
        if (event.key === "Enter") (event.preventDefault(), finish("enter"));
        else if (event.key === "Escape") (event.preventDefault(), event.stopPropagation(), finish("escape"));
        else if (event.key === "Tab") (event.preventDefault(), finish(event.shiftKey ? "shift-tab" : "tab"));
        else event.stopPropagation();
      }}
      onBlur={() => finish("blur")}
    />
  );
}

/** One cell: its value, or the editor that fits its type while it is being edited. */
export function EditableCell(props: EditableCellProps): JSX.Element {
  const typeOf = () => (props.property === "title" ? "title" : props.property.type);
  const close = () => props.onExit("escape");
  const popoverProperty = () => (props.property === "title" ? null : props.property);
  return (
    <>
      <Show when={props.editing && isTextLike(props.property)} fallback={<CellDisplay property={props.property} value={props.value} title={props.title} titles={props.titles} />}>
        <TextEditor {...props} />
      </Show>
      <Show when={props.editing ? props.anchor() : undefined}>
        {(anchor) => (
          <Switch>
            <Match when={typeOf() === "select" || typeOf() === "multi_select"}>
              <SelectEditor
                anchor={anchor()}
                property={popoverProperty() as NonNullable<ReturnType<typeof popoverProperty>>}
                value={props.value}
                onChange={props.onChange}
                onClose={close}
                onManageOptions={props.onManageOptions}
              />
            </Match>
            <Match when={typeOf() === "date"}>
              <DateEditor anchor={anchor()} value={props.value} onChange={props.onChange} onClose={close} />
            </Match>
            <Match when={typeOf() === "relation"}>
              <RelationPicker
                anchor={anchor()}
                property={popoverProperty() as NonNullable<ReturnType<typeof popoverProperty>>}
                value={props.value}
                titles={props.titles}
                onChange={props.onChange}
                onClose={close}
              />
            </Match>
          </Switch>
        )}
      </Show>
    </>
  );
}
