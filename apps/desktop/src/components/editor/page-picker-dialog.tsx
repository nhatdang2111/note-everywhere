import { For, Show, createMemo, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Dialog } from "../common/dialog.tsx";
import { Icon } from "../common/icons.tsx";
import { pageCandidates } from "./page-candidates.ts";

export interface PagePickerDialogProps {
  /** The page being edited; it is not offered as a link target. */
  excludeId?: string;
  onPick(page: { id: string; title: string }): void;
  onCancel(): void;
}

/** Choose a page from a list: type to filter, arrow keys to move, Enter to choose. */
export function PagePickerDialog(props: PagePickerDialogProps): JSX.Element {
  const app = useApp();
  const [query, setQuery] = createSignal("");
  const [index, setIndex] = createSignal(0);
  const candidates = createMemo(() =>
    pageCandidates(app.tree.index(), query(), { exclude: props.excludeId, limit: 50, recent: app.recents.ids(), untitled: t("tree.untitled") }),
  );

  const choose = (at: number) => {
    const page = candidates()[at];
    if (page) props.onPick({ id: page.id, title: app.tree.title(page.id) });
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const count = candidates().length;
    if (event.key === "ArrowDown" && count) (event.preventDefault(), setIndex((index() + 1) % count));
    else if (event.key === "ArrowUp" && count) (event.preventDefault(), setIndex((index() + count - 1) % count));
    else if (event.key === "Enter") (event.preventDefault(), choose(index()));
  };

  return (
    <Dialog title={t("picker.dialogTitle")} onClose={props.onCancel}>
      <input
        class="picker-input"
        type="search"
        role="combobox"
        aria-expanded="true"
        aria-controls="picker-list"
        aria-label={t("picker.searchLabel")}
        aria-activedescendant={candidates()[index()] ? `picker-${candidates()[index()]?.id}` : undefined}
        placeholder={t("picker.searchPlaceholder")}
        data-autofocus
        value={query()}
        onInput={(event) => (setQuery(event.currentTarget.value), setIndex(0))}
        onKeyDown={onKeyDown}
      />
      <ul id="picker-list" class="picker-list" role="listbox" aria-label={t("picker.heading")}>
        <For each={candidates()}>
          {(page, at) => (
            <li id={`picker-${page.id}`} role="option" class="suggestion-row" aria-selected={index() === at()} onMouseEnter={() => setIndex(at())} onClick={() => choose(at())}>
              <Icon name="page" />
              <span class="suggestion-text">
                <span class="suggestion-label">{page.title}</span>
                <Show when={page.path}>
                  <span class="suggestion-hint">{page.path}</span>
                </Show>
              </span>
            </li>
          )}
        </For>
      </ul>
      <Show when={candidates().length === 0}>
        <p class="suggestion-empty">{t("picker.noMatch")}</p>
      </Show>
    </Dialog>
  );
}
