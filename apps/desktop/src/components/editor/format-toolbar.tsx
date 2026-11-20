import { Show, createEffect, createMemo, createSignal, on, onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import { TextSelection } from "prosemirror-state";
import type { Command } from "prosemirror-state";
import {
  isMarkActive,
  isSafeHref,
  linkAt,
  normalizeHref,
  pageSchema,
  removeLink,
  setLink,
  toggleBold,
  toggleCode,
  toggleItalic,
  toggleStrike,
  type PageEditor,
} from "../../editor/index.ts";
import { t } from "../../i18n/index.ts";
import { modLabel } from "../../lib/device.ts";
import { Button, IconButton } from "../common/button.tsx";
import type { IconName } from "../common/icons.tsx";

export interface FormatToolbarProps {
  editor: PageEditor;
  /** Changes whenever the editor state changes. */
  tick: () => number;
  /** Changes when the link shortcut is pressed. */
  linkRequest: () => number;
  onOpenExternal(href: string): void;
}

interface Placement {
  left: number;
  top: number;
}

const { marks } = pageSchema;

/**
 * Floating bar over selected text (bold, italic, code, strike, link) and a
 * small bar over a link under the cursor. Both share the link editor.
 */
export function FormatToolbar(props: FormatToolbarProps): JSX.Element {
  const [editing, setEditing] = createSignal(false);
  const [href, setHref] = createSignal("");
  const [error, setError] = createSignal(false);
  const [focused, setFocused] = createSignal(false);
  const [frozen, setFrozen] = createSignal<Placement | null>(null);
  const editor = props.editor;
  const view = () => editor.view;

  onMount(() => {
    const dom = view().dom;
    const sync = () => setFocused(view().hasFocus());
    dom.addEventListener("focusin", sync);
    dom.addEventListener("focusout", sync);
    onCleanup(() => {
      dom.removeEventListener("focusin", sync);
      dom.removeEventListener("focusout", sync);
    });
  });

  const snapshot = createMemo(() => {
    props.tick();
    const state = view().state;
    const { selection } = state;
    const inCode = Boolean(selection.$from.parent.type.spec.code);
    const range = !selection.empty && selection instanceof TextSelection && !inCode;
    const link = linkAt(state);
    const showLink = selection.empty && link !== null && !inCode;
    if (!range && !showLink) return null;
    const start = view().coordsAtPos(selection.from);
    const end = view().coordsAtPos(selection.to);
    const placement: Placement = { left: (start.left + end.left) / 2, top: start.top < 56 ? end.bottom + 8 : start.top - 8 };
    return {
      placement,
      below: start.top < 56,
      href: link ? String(link.mark.attrs.href) : "",
      range,
      active: {
        bold: isMarkActive(state, marks.bold),
        italic: isMarkActive(state, marks.italic),
        code: isMarkActive(state, marks.code),
        strike: isMarkActive(state, marks.strike),
        link: link !== null,
      },
    };
  });

  const startEditing = () => {
    const current = snapshot();
    if (!current) return false;
    setFrozen(current.placement);
    setHref(current.href);
    setError(false);
    setEditing(true);
    return true;
  };

  createEffect(on(props.linkRequest, () => void startEditing(), { defer: true }));

  const stopEditing = () => {
    setEditing(false);
    setFrozen(null);
    view().focus();
  };

  const run = (command: Command) => {
    command(view().state, view().dispatch);
    view().focus();
  };

  const apply = () => {
    const normalized = normalizeHref(href());
    if (!normalized) return setError(true);
    run(setLink(normalized));
    stopEditing();
  };

  const visible = () => (editing() ? frozen() !== null : snapshot() !== null && focused());
  const placement = () => (editing() ? frozen() : snapshot()?.placement) ?? { left: 0, top: 0 };

  const button = (icon: IconName, label: string, key: keyof NonNullable<ReturnType<typeof snapshot>>["active"], action: () => void, shortcut?: string) => (
    <IconButton
      icon={icon}
      label={shortcut ? `${label} (${modLabel()}${shortcut})` : label}
      pressed={snapshot()?.active[key] ?? false}
      onMouseDown={(event) => event.preventDefault()}
      onClick={action}
    />
  );

  return (
    <Show when={visible()}>
      <Portal>
        <div
          class="format-toolbar"
          role="toolbar"
          aria-label={t("format.label")}
          data-below={snapshot()?.below ? "true" : undefined}
          style={{ left: `${placement().left}px`, top: `${placement().top}px` }}
          onMouseDown={(event) => {
            if (!(event.target instanceof HTMLInputElement)) event.preventDefault();
          }}
        >
          <Show
            when={!editing()}
            fallback={
              <form
                class="link-editor"
                onSubmit={(event) => {
                  event.preventDefault();
                  apply();
                }}
              >
                <input
                  type="text"
                  aria-label={t("format.linkAddress")}
                  aria-invalid={error() ? "true" : undefined}
                  placeholder={t("format.linkPlaceholder")}
                  value={href()}
                  ref={(element) => queueMicrotask(() => element.focus())}
                  onInput={(event) => (setHref(event.currentTarget.value), setError(false))}
                  onKeyDown={(event) => {
                    if (event.key === "Escape") {
                      event.preventDefault();
                      event.stopPropagation();
                      stopEditing();
                    }
                  }}
                />
                <Button variant="primary" type="submit">
                  {t("format.apply")}
                </Button>
                <Show when={snapshot()?.active.link}>
                  <Button
                    onClick={() => {
                      run(removeLink);
                      stopEditing();
                    }}
                  >
                    {t("format.removeLink")}
                  </Button>
                </Show>
                <Show when={error()}>
                  <span class="field-error" role="alert">
                    {t("format.linkInvalid")}
                  </span>
                </Show>
              </form>
            }
          >
            <Show
              when={snapshot()?.range}
              fallback={
                <div class="link-view">
                  <span class="link-view-href" title={snapshot()?.href}>
                    {snapshot()?.href}
                  </span>
                  <Show when={isSafeHref(snapshot()?.href ?? "") && !(snapshot()?.href ?? "").startsWith("ne-page:")}>
                    <Button onClick={() => props.onOpenExternal(snapshot()?.href ?? "")}>{t("format.openLink")}</Button>
                  </Show>
                  <Button onClick={() => void startEditing()}>{t("format.editLink")}</Button>
                  <Button onClick={() => run(removeLink)}>{t("format.removeLink")}</Button>
                </div>
              }
            >
              {button("bold", t("format.bold"), "bold", () => run(toggleBold), "B")}
              {button("italic", t("format.italic"), "italic", () => run(toggleItalic), "I")}
              {button("code", t("format.code"), "code", () => run(toggleCode), "E")}
              {button("strike", t("format.strike"), "strike", () => run(toggleStrike), "⇧S")}
              {button("link", t("format.link"), "link", () => void startEditing(), "⇧K")}
            </Show>
          </Show>
        </div>
      </Portal>
    </Show>
  );
}
