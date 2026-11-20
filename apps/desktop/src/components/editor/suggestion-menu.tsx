import { For, Show, createEffect, createMemo, createSignal, createUniqueId, on, onCleanup, onMount, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import {
  applyBlockSuggestion,
  applyPageSuggestion,
  attachmentSrc,
  insertChoice,
  type BlockChoice,
  type PageEditor,
  type SuggestionState,
} from "../../editor/index.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Icon, type IconName } from "../common/icons.tsx";
import { pageCandidates } from "./page-candidates.ts";
import { filterSlashItems, type SlashItem } from "./slash-items.ts";

export type KeyHandler = (event: KeyboardEvent) => boolean;

export interface SuggestionMenuProps {
  state: SuggestionState;
  editor: PageEditor;
  pageId: string;
  /** Ask for a page in a dialog; resolves to null when cancelled. */
  requestPage(): Promise<{ id: string; title: string } | null>;
  registerKeys(handler: KeyHandler | null): void;
  /** Choose an existing synced block to insert after the current block. */
  onInsertSynced(): void;
}

interface Entry {
  key: string;
  icon: IconName;
  label: string;
  hint?: string;
  run(): void | Promise<void>;
}

const MENU_HEIGHT = 340;

/** The `/` block menu and the `[[` / `@` page picker: one filtered list with arrow-key selection. */
export function SuggestionMenu(props: SuggestionMenuProps): JSX.Element {
  const app = useApp();
  const listId = createUniqueId();
  const [index, setIndex] = createSignal(0);
  // Taken once: the menu's async actions can outlive it, and props must not be read after it unmounts.
  const editor = props.editor;
  const pageId = props.pageId;
  const requestPage = props.requestPage;
  const view = () => editor.view;

  const deleteTrigger = (match: SuggestionState) => view().dispatch(view().state.tr.delete(match.from, match.to));

  const runSlash = async (item: SlashItem, match: SuggestionState): Promise<void> => {
    const { id } = item;
    if (id === "image") {
      deleteTrigger(match);
      const added = await app.guard(() => app.core.api.attachmentPick({ imagesOnly: true }));
      if (added) insertChoice("image", { src: attachmentSrc(added.hash) })(view().state, view().dispatch);
    } else if (id === "child_page") {
      deleteTrigger(match);
      const child = await app.guard(() => app.tree.create({ kind: "page", parentId: pageId }));
      if (child) insertChoice("child_page", { pageId: child, title: app.tree.title(child) })(view().state, view().dispatch);
    } else if (id === "database") {
      deleteTrigger(match);
      const database = await app.guard(async () => {
        const created = await app.core.api.dbCreate({ parentId: pageId });
        await app.tree.refresh();
        app.tree.setExpanded(pageId, true);
        return created;
      });
      if (database) insertChoice("child_page", { pageId: database, title: app.tree.title(database) })(view().state, view().dispatch);
    } else if (id === "synced_existing") {
      deleteTrigger(match);
      props.onInsertSynced();
      return;
    } else if (id === "page_link") {
      deleteTrigger(match);
      const picked = await requestPage();
      if (picked) insertChoice("page_link", { pageId: picked.id, title: picked.title })(view().state, view().dispatch);
    } else {
      applyBlockSuggestion(match, id as BlockChoice)(view().state, view().dispatch, view());
    }
    view().focus();
  };

  const entries = createMemo<Entry[]>(() => {
    const { trigger, query } = props.state;
    if (trigger === "slash") {
      return filterSlashItems(query, t).map((item) => ({
        key: item.id,
        icon: item.icon,
        label: t(item.label),
        hint: t(item.hint),
        run: () => runSlash(item, { ...props.state }),
      }));
    }
    const untitled = t("tree.untitled");
    const found: Entry[] = pageCandidates(app.tree.index(), query, { exclude: pageId, limit: 8, recent: app.recents.ids(), untitled }).map((page) => ({
      key: page.id,
      icon: "page",
      label: page.title,
      hint: page.path || undefined,
      run: () => {
        applyPageSuggestion({ ...props.state }, { id: page.id, title: app.tree.title(page.id) })(view().state, view().dispatch);
        view().focus();
      },
    }));
    const typed = query.trim();
    if (typed && !found.some((entry) => entry.label.toLowerCase() === typed.toLowerCase())) {
      found.push({
        key: "create",
        icon: "plus",
        label: t("picker.create", { title: typed }),
        run: async () => {
          const match = { ...props.state };
          const id = await app.guard(() => app.tree.create({ kind: "page", parentId: null, title: typed }));
          if (id) applyPageSuggestion(match, { id, title: typed })(view().state, view().dispatch);
          view().focus();
        },
      });
    }
    return found;
  });

  // The highlighted row restarts at the top whenever the list changes.
  createEffect(on(() => props.state.query + props.state.trigger, () => setIndex(0), { defer: true }));

  const accept = (entry: Entry | undefined) => {
    if (entry) void entry.run();
  };

  const onKey: KeyHandler = (event) => {
    const list = entries();
    switch (event.key) {
      case "ArrowDown":
        if (list.length > 0) setIndex((index() + 1) % list.length);
        break;
      case "ArrowUp":
        if (list.length > 0) setIndex((index() + list.length - 1) % list.length);
        break;
      case "Enter":
      case "Tab":
        if (list.length === 0) return false;
        accept(list[index()]);
        break;
      case "Escape":
        editor.dismissSuggestion();
        break;
      default:
        return false;
    }
    event.preventDefault();
    return true;
  };

  onMount(() => props.registerKeys(onKey));
  onCleanup(() => props.registerKeys(null));

  // Tell assistive technology which row is highlighted while focus stays in the editor.
  const optionId = (key: string) => `${listId}-${key}`;
  createEffect(() => {
    const active = entries()[index()];
    const dom = view().dom;
    dom.setAttribute("aria-controls", listId);
    if (active) dom.setAttribute("aria-activedescendant", optionId(active.key));
    else dom.removeAttribute("aria-activedescendant");
  });
  onCleanup(() => {
    view().dom.removeAttribute("aria-controls");
    view().dom.removeAttribute("aria-activedescendant");
  });

  const position = createMemo(() => {
    const anchor = props.state.anchor;
    if (!anchor) return { left: 80, top: 120 };
    const below = anchor.bottom + 6;
    const flip = below + MENU_HEIGHT > window.innerHeight && anchor.top > MENU_HEIGHT;
    return { left: Math.min(anchor.left, window.innerWidth - 300), top: flip ? anchor.top - MENU_HEIGHT - 6 : below };
  });

  return (
    <Portal>
      <div
        class="suggestion-menu"
        style={{ left: `${position().left}px`, top: `${position().top}px` }}
        // Keep the editor focused: clicking a row must not blur it.
        onMouseDown={(event) => event.preventDefault()}
      >
        <div class="menu-heading">{props.state.trigger === "slash" ? t("slash.heading") : t("picker.heading")}</div>
        <ul id={listId} role="listbox" aria-label={props.state.trigger === "slash" ? t("slash.heading") : t("picker.heading")}>
          <For each={entries()}>
            {(entry, at) => (
              <li
                id={optionId(entry.key)}
                role="option"
                class="suggestion-row"
                aria-selected={index() === at()}
                onMouseEnter={() => setIndex(at())}
                onClick={() => accept(entry)}
              >
                <Icon name={entry.icon} />
                <span class="suggestion-text">
                  <span class="suggestion-label">{entry.label}</span>
                  <Show when={entry.hint}>
                    <span class="suggestion-hint">{entry.hint}</span>
                  </Show>
                </span>
              </li>
            )}
          </For>
        </ul>
        <Show when={entries().length === 0}>
          <p class="suggestion-empty">{props.state.trigger === "slash" ? t("slash.noMatch") : t("picker.noMatch")}</p>
        </Show>
      </div>
    </Portal>
  );
}
