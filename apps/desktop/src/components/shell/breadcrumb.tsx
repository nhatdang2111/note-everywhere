import { For, Show, type JSX } from "solid-js";
import { t, type MessageKey } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { ancestors } from "../../state/tree-model.ts";

const TARGET_LABEL: Record<string, MessageKey> = {
  home: "tabs.home",
  trash: "tabs.trash",
  vault: "tabs.vault",
  settings: "tabs.settings",
};

/** Where the open page sits in the tree. Page ancestors are links; folders are labels. */
export function Breadcrumb(): JSX.Element {
  const app = useApp();
  const trail = () => {
    const pageId = app.tabs.activePageId();
    if (!pageId) return [];
    return ancestors(app.tree.index(), pageId);
  };
  const current = () => {
    const target = app.tabs.target();
    if (target.kind === "page") return app.tree.title(target.pageId) || t("tree.untitled");
    return t(TARGET_LABEL[target.kind] ?? "tabs.home");
  };
  return (
    <nav class="breadcrumb" aria-label={t("breadcrumb.label")}>
      <ol>
        <For each={trail()}>
          {(node) => (
            <li>
              <Show when={node.kind === "page"} fallback={<span class="crumb-static">{node.title || t("tree.untitled")}</span>}>
                <button type="button" class="crumb" onClick={() => app.openPage(node.id)}>
                  {node.title || t("tree.untitled")}
                </button>
              </Show>
              <span class="crumb-sep" aria-hidden="true">
                /
              </span>
            </li>
          )}
        </For>
        <li aria-current="page">
          <span class="crumb-current">{current()}</span>
        </li>
      </ol>
    </nav>
  );
}
