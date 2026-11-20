import { Show, createSignal, createUniqueId, splitProps, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { IconButton } from "./button.tsx";

export interface TextFieldProps extends Omit<JSX.InputHTMLAttributes<HTMLInputElement>, "onInput" | "value" | "id"> {
  label: string;
  value: string;
  onInput(value: string): void;
  hint?: JSX.Element;
  error?: string | null;
  /** Marks the input that should receive focus when a dialog opens. */
  autofocus?: boolean;
}

export function TextField(props: TextFieldProps): JSX.Element {
  const id = createUniqueId();
  const [own, rest] = splitProps(props, ["label", "value", "onInput", "hint", "error", "autofocus", "class"]);
  return (
    <div class={`field ${own.class ?? ""}`}>
      <label for={id}>{own.label}</label>
      <input
        {...rest}
        id={id}
        value={own.value}
        data-autofocus={own.autofocus ? "" : undefined}
        aria-invalid={own.error ? "true" : undefined}
        aria-describedby={own.hint || own.error ? `${id}-note` : undefined}
        onInput={(event) => own.onInput(event.currentTarget.value)}
      />
      <Show when={own.error || own.hint}>
        <p id={`${id}-note`} class={own.error ? "field-error" : "field-hint"}>
          {own.error ?? own.hint}
        </p>
      </Show>
    </div>
  );
}

/** Password input with a show/hide toggle. The toggle never reveals a stored secret on its own. */
export function PasswordField(props: Omit<TextFieldProps, "type">): JSX.Element {
  const [shown, setShown] = createSignal(false);
  return (
    <div class="password-field">
      <TextField {...props} type={shown() ? "text" : "password"} autocomplete={props.autocomplete ?? "off"} spellcheck={false} />
      <IconButton
        class="password-toggle"
        icon={shown() ? "eye-off" : "eye"}
        label={shown() ? t("field.hidePassword") : t("field.showPassword")}
        pressed={shown()}
        onClick={() => setShown(!shown())}
      />
    </div>
  );
}

export interface CheckboxProps {
  label: JSX.Element;
  checked: boolean;
  onChange(checked: boolean): void;
  disabled?: boolean;
}

export function Checkbox(props: CheckboxProps): JSX.Element {
  return (
    <label class="checkbox">
      <input type="checkbox" checked={props.checked} disabled={props.disabled} onChange={(event) => props.onChange(event.currentTarget.checked)} />
      <span>{props.label}</span>
    </label>
  );
}

export interface SelectProps<T extends string> {
  label: string;
  value: T;
  options: { value: T; label: string }[];
  onChange(value: T): void;
  hint?: string;
}

export function Select<T extends string>(props: SelectProps<T>): JSX.Element {
  const id = createUniqueId();
  return (
    <div class="field">
      <label for={id}>{props.label}</label>
      <select id={id} value={props.value} onChange={(event) => props.onChange(event.currentTarget.value as T)}>
        {props.options.map((option) => (
          <option value={option.value} selected={option.value === props.value}>
            {option.label}
          </option>
        ))}
      </select>
      <Show when={props.hint}>
        <p class="field-hint">{props.hint}</p>
      </Show>
    </div>
  );
}
