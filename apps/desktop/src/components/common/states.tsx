import { Show, type JSX } from "solid-js";
import type { ApiError } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { Icon, type IconName } from "./icons.tsx";

export function Spinner(props: { label?: string }): JSX.Element {
  return (
    <span class="spinner" role="status">
      <span class="spinner-ring" aria-hidden="true" />
      <span class="spinner-label">{props.label ?? t("common.loading")}</span>
    </span>
  );
}

/** A failed command or load: shows `ApiError.message` and an optional retry. */
export function ErrorBanner(props: { error: ApiError | string | null | undefined; children?: JSX.Element }): JSX.Element {
  const message = () => (typeof props.error === "string" ? props.error : props.error?.message);
  return (
    <Show when={message()}>
      <div class="error-banner" role="alert">
        <Icon name="alert" />
        <span>{message()}</span>
        {props.children}
      </div>
    </Show>
  );
}

/** Explains why there is nothing to show and what to do next. */
export function EmptyState(props: { icon?: IconName; title: string; children?: JSX.Element; action?: JSX.Element }): JSX.Element {
  return (
    <div class="empty-state">
      <Show when={props.icon}>{(name) => <Icon name={name()} size={28} />}</Show>
      <h3>{props.title}</h3>
      <Show when={props.children}>
        <p>{props.children}</p>
      </Show>
      {props.action}
    </div>
  );
}
