import { For, Show, createEffect, createSignal, on, onCleanup, type JSX } from "solid-js";
import type { ApiError, Backlink } from "../../api/contract.ts";
import { toApiError } from "../../api/errors.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Icon } from "../common/icons.tsx";
import { ErrorBanner } from "../common/states.tsx";

/** Pages that link to this one. Refreshes when the workspace changes. */
export function BacklinksPanel(props: { pageId: string }): JSX.Element {
  const app = useApp();
  const [links, setLinks] = createSignal<Backlink[] | null>(null);
  const [error, setError] = createSignal<ApiError | null>(null);
  let version = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;

  const load = async () => {
    const mine = (version += 1);
    try {
      const result = await app.core.api.pageBacklinks({ id: props.pageId });
      if (mine === version) (setLinks(result), setError(null));
    } catch (failure) {
      if (mine === version) setError(toApiError(failure));
    }
  };

  // Load now, and again shortly after the tree changes (titles, new links, deletions).
  createEffect(
    on(
      () => app.tree.nodes(),
      () => {
        clearTimeout(timer);
        timer = setTimeout(() => void load(), 250);
      },
    ),
  );
  onCleanup(() => {
    clearTimeout(timer);
    version += 1;
  });

  return (
    <section class="backlinks" aria-labelledby="backlinks-title">
      <h2 id="backlinks-title" class="section-title">
        {t("backlinks.title")}
        <Show when={links()}>
          <span class="count-badge">{links()?.length}</span>
        </Show>
      </h2>
      <ErrorBanner error={error()} />
      <Show when={links()} fallback={<Show when={!error()}>{<p class="muted">{t("common.loading")}</p>}</Show>}>
        {(list) => (
          <Show when={list().length > 0} fallback={<p class="muted">{t("backlinks.empty")}</p>}>
            <ul class="backlink-list">
              <For each={list()}>
                {(link) => (
                  <li>
                    <button type="button" class="backlink" onClick={(event) => app.openPage(link.pageId, event.metaKey || event.ctrlKey)}>
                      <Icon name="page-link" />
                      <span class="backlink-text">
                        <span class="backlink-title">{link.title || t("tree.untitled")}</span>
                        <Show when={link.snippet}>
                          <span class="backlink-snippet">{link.snippet}</span>
                        </Show>
                      </span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </Show>
        )}
      </Show>
    </section>
  );
}
