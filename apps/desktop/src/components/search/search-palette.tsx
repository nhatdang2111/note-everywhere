import { For, Show, createMemo, createSignal, createUniqueId, onCleanup, type JSX } from "solid-js";
import { Portal } from "solid-js/web";
import type { ApiError, NodeId } from "../../api/contract.ts";
import { toApiError } from "../../api/errors.ts";
import { t } from "../../i18n/index.ts";
import { splitHighlights } from "../../lib/highlight.ts";
import { useApp } from "../../state/app.tsx";
import { ancestors } from "../../state/tree-model.ts";
import { Icon } from "../common/icons.tsx";
import { ErrorBanner } from "../common/states.tsx";

const DEBOUNCE_MS = 150;

interface PageEntry {
  kind: "page";
  key: string;
  pageId: NodeId;
  title: string;
  breadcrumb: string[];
  snippet?: string;
  highlights?: [number, number][];
}
interface CreateEntry {
  kind: "create";
  key: string;
  title: string;
}
type Entry = PageEntry | CreateEntry;

function Highlighted(props: { text: string; ranges: [number, number][] }): JSX.Element {
  return (
    <>
      <For each={splitHighlights(props.text, props.ranges)}>{(run) => (run.mark ? <mark>{run.text}</mark> : <>{run.text}</>)}</For>
    </>
  );
}

/** Quick search (Cmd/Ctrl+K): recent pages when empty, results while typing, create a page from the query. */
export function SearchPalette(props: { onClose(): void }): JSX.Element {
  const app = useApp();
  const listId = createUniqueId();
  const opener = document.activeElement as HTMLElement | null;
  const [query, setQuery] = createSignal("");
  const [hits, setHits] = createSignal<PageEntry[]>([]);
  const [searching, setSearching] = createSignal(false);
  const [error, setError] = createSignal<ApiError | null>(null);
  const [index, setIndex] = createSignal(0);
  let timer: ReturnType<typeof setTimeout> | undefined;
  let request = 0;

  onCleanup(() => {
    clearTimeout(timer);
    request += 1;
    opener?.focus?.();
  });

  const recent = createMemo<PageEntry[]>(() => {
    const treeIndex = app.tree.index();
    return app.recents
      .ids()
      .filter((id) => treeIndex.byId.has(id))
      .map((id) => ({
        kind: "page" as const,
        key: id,
        pageId: id,
        title: app.tree.title(id) || t("tree.untitled"),
        breadcrumb: ancestors(treeIndex, id).map((node) => node.title || t("tree.untitled")),
      }));
  });

  const entries = createMemo<Entry[]>(() => {
    const text = query().trim();
    if (!text) return recent();
    const list: Entry[] = [...hits()];
    list.push({ kind: "create", key: "create", title: text });
    return list;
  });

  const run = async (text: string) => {
    const mine = (request += 1);
    setSearching(true);
    try {
      const found = await app.core.api.searchQuery({ text, limit: 20 });
      if (mine !== request) return;
      setHits(
        found.map((hit) => ({
          kind: "page" as const,
          key: hit.pageId,
          pageId: hit.pageId,
          title: hit.title || t("tree.untitled"),
          breadcrumb: hit.breadcrumb,
          snippet: hit.snippet,
          highlights: hit.highlights,
        })),
      );
      setError(null);
    } catch (failure) {
      if (mine === request) setError(toApiError(failure));
    } finally {
      if (mine === request) setSearching(false);
    }
  };

  const onInput = (value: string) => {
    setQuery(value);
    setIndex(0);
    clearTimeout(timer);
    const text = value.trim();
    if (!text) {
      request += 1;
      setSearching(false);
      setHits([]);
      setError(null);
      return;
    }
    timer = setTimeout(() => void run(text), DEBOUNCE_MS);
  };

  const choose = async (entry: Entry | undefined, inNewTab: boolean) => {
    if (!entry) return;
    if (entry.kind === "page") {
      props.onClose();
      app.openPage(entry.pageId, inNewTab);
      return;
    }
    const id = await app.guard(() => app.tree.create({ kind: "page", parentId: null, title: entry.title }));
    if (id) {
      props.onClose();
      app.openPage(id, inNewTab);
    }
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const count = entries().length;
    switch (event.key) {
      case "ArrowDown":
        event.preventDefault();
        if (count) setIndex((index() + 1) % count);
        break;
      case "ArrowUp":
        event.preventDefault();
        if (count) setIndex((index() + count - 1) % count);
        break;
      case "Home":
        event.preventDefault();
        setIndex(0);
        break;
      case "End":
        event.preventDefault();
        setIndex(Math.max(0, count - 1));
        break;
      case "Enter":
        event.preventDefault();
        void choose(entries()[index()], event.metaKey || event.ctrlKey);
        break;
      case "Escape":
        event.preventDefault();
        event.stopPropagation();
        props.onClose();
        break;
    }
  };

  const optionId = (entry: Entry) => `${listId}-${entry.key}`;

  return (
    <Portal>
      <div class="dialog-backdrop palette-backdrop" onMouseDown={(event) => event.target === event.currentTarget && props.onClose()}>
        <div class="palette" role="dialog" aria-modal="true" aria-label={t("search.title")}>
          <div class="palette-input">
            <Icon name="search" />
            <input
              type="text"
              role="combobox"
              aria-expanded="true"
              aria-controls={listId}
              aria-autocomplete="list"
              aria-activedescendant={entries()[index()] ? optionId(entries()[index()] as Entry) : undefined}
              aria-label={t("search.title")}
              placeholder={t("search.placeholder")}
              value={query()}
              ref={(element) => queueMicrotask(() => element.focus())}
              onInput={(event) => onInput(event.currentTarget.value)}
              onKeyDown={onKeyDown}
            />
          </div>
          <ErrorBanner error={error()} />
          <div class="palette-section">
            <Show when={!query().trim()}>
              <div class="menu-heading">{t("search.recent")}</div>
            </Show>
            <ul id={listId} role="listbox" aria-label={t("search.results")} class="palette-list">
              <For each={entries()}>
                {(entry, at) => (
                  <li
                    id={optionId(entry)}
                    role="option"
                    class="palette-row"
                    aria-selected={index() === at()}
                    onMouseEnter={() => setIndex(at())}
                    onClick={(event) => void choose(entry, event.metaKey || event.ctrlKey)}
                  >
                    <Show
                      when={entry.kind === "page" && entry}
                      fallback={
                        <>
                          <Icon name="plus" />
                          <span class="palette-text">
                            <span class="palette-title">{t("search.create", { title: entry.title })}</span>
                          </span>
                        </>
                      }
                    >
                      {(page) => (
                        <>
                          <Icon name="page" />
                          <span class="palette-text">
                            <span class="palette-title">{page().title}</span>
                            <Show when={page().breadcrumb.length > 0}>
                              <span class="palette-crumb">{page().breadcrumb.join(" / ")}</span>
                            </Show>
                            <Show when={page().snippet}>
                              <span class="palette-snippet">
                                <Highlighted text={page().snippet ?? ""} ranges={page().highlights ?? []} />
                              </span>
                            </Show>
                          </span>
                        </>
                      )}
                    </Show>
                  </li>
                )}
              </For>
            </ul>
            <Show when={query().trim() && !searching() && hits().length === 0 && !error()}>
              <p class="suggestion-empty">{t("search.noResults")}</p>
            </Show>
            <Show when={!query().trim() && recent().length === 0}>
              <p class="suggestion-empty">{t("search.noRecent")}</p>
            </Show>
            <Show when={searching()}>
              <p class="suggestion-empty" role="status">
                {t("search.searching")}
              </p>
            </Show>
          </div>
        </div>
      </div>
    </Portal>
  );
}
