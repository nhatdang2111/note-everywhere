import { splitProps, type JSX } from "solid-js";
import { Icon, type IconName } from "./icons.tsx";

type Variant = "primary" | "secondary" | "ghost" | "danger";

export interface ButtonProps extends JSX.ButtonHTMLAttributes<HTMLButtonElement> {
  variant?: Variant;
  /** Shows progress; the button stays the same size and ignores clicks. */
  busy?: boolean;
  icon?: IconName;
}

export function Button(props: ButtonProps): JSX.Element {
  const [own, rest] = splitProps(props, ["variant", "busy", "icon", "children", "class", "disabled", "type"]);
  return (
    <button
      {...rest}
      type={own.type ?? "button"}
      class={`btn btn-${own.variant ?? "secondary"} ${own.class ?? ""}`}
      disabled={own.disabled || own.busy}
      aria-busy={own.busy ? "true" : undefined}
    >
      {own.icon && <Icon name={own.icon} />}
      {own.children}
    </button>
  );
}

export interface IconButtonProps extends Omit<JSX.ButtonHTMLAttributes<HTMLButtonElement>, "children"> {
  icon: IconName;
  /** Required: an icon-only button needs an accessible name. */
  label: string;
  pressed?: boolean;
  filled?: boolean;
  size?: number;
}

export function IconButton(props: IconButtonProps): JSX.Element {
  const [own, rest] = splitProps(props, ["icon", "label", "pressed", "filled", "size", "class", "type"]);
  return (
    <button
      {...rest}
      type={own.type ?? "button"}
      class={`icon-btn ${own.class ?? ""}`}
      aria-label={own.label}
      title={own.label}
      aria-pressed={own.pressed === undefined ? undefined : own.pressed}
    >
      <Icon name={own.icon} size={own.size} filled={own.filled} />
    </button>
  );
}
