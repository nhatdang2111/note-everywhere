import { For, Show, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { modLabel } from "../../lib/device.ts";
import { useApp } from "../../state/app.tsx";
import type { Target } from "../../state/tabs-model.ts";
import { IconButton } from "../common/button.tsx";
import { Icon, type IconName } from "../common/icons.tsx";
import { Menu, type MenuEntry } from "../common/menu.tsx";
import { ErrorBanner, Spinner } from "../common/states.tsx";
import { TreeView } from "./tree-view.tsx";

function NavButton(props: { icon: IconName; label: string; target: Target }): JSX.Element {
  const app = useApp();
  const current = () => app.tabs.target().kind === props.target.kind;
  return (
    <button type="button" class="nav-button" aria-current={current() ? "page" : undefined} onClick={() => app.openTarget(props.target)}>
      <Icon name={props.icon} />
      <span>{props.label}</span>
    </button>
  );
}

export function Sidebar(): JSX.Element {
  const app = useApp();
  const { tree, tabs, ui } = app;
  const [menuAt, setMenuAt] = createSignal<{ x: number; y: number } | null>(null);

  const workspaceEntries = (): MenuEntry[] => [
    { kind: "item", icon: "page", label: t("sidebar.newPage"), onSelect: () => void app.createNode("page", null) },
    { kind: "item", icon: "folder", label: t("sidebar.newFolder"), onSelect: () => void app.createNode("folder", null) },
    { kind: "separator" },
    { kind: "item", icon: "download", label: t("sidebar.importMarkdown"), onSelect: () => void app.importMarkdown(null) },
    { kind: "item", icon: "upload", label: t("sidebar.exportAll"), onSelect: () => void app.exportMarkdown(null) },
  ];

  return (
    <nav class="sidebar" aria-label={t("sidebar.label")}>
      <div class="sidebar-header">
        <button type="button" class="search-button" onClick={() => ui.setSearchOpen(true)}>
          <Icon name="search" />
          <span>{t("sidebar.search")}</span>
          <kbd>{modLabel()}K</kbd>
        </button>
        <IconButton
          icon="more"
          label={t("sidebar.workspaceMenu")}
          onClick={(event) => {
            const rect = event.currentTarget.getBoundingClientRect();
            setMenuAt({ x: rect.left, y: rect.bottom + 4 });
          }}
        />
      </div>

      <div class="sidebar-scroll">
        <Show when={tree.favorites().length > 0}>
          <section aria-labelledby="favorites-heading">
            <h2 id="favorites-heading" class="sidebar-heading">
              {t("sidebar.favorites")}
            </h2>
            <ul class="favorite-list">
              <For each={tree.favorites()}>
                {(node) => (
                  <li>
                    <button
                      type="button"
                      class="nav-button"
                      aria-current={tabs.activePageId() === node.id ? "page" : undefined}
                      onClick={(event) => (node.kind === "page" ? app.openPage(node.id, event.metaKey || event.ctrlKey) : tree.setExpanded(node.id, true))}
                    >
                      <Icon name="star" filled />
                      <span class="nav-label">{node.title || t("tree.untitled")}</span>
                    </button>
                  </li>
                )}
              </For>
            </ul>
          </section>
        </Show>

        <section aria-labelledby="pages-heading">
          <div class="sidebar-heading-row">
            <h2 id="pages-heading" class="sidebar-heading">
              {t("sidebar.pages")}
            </h2>
            <IconButton icon="page" size={14} label={t("sidebar.newPage")} onClick={() => void app.createNode("page", null)} />
            <IconButton icon="folder" size={14} label={t("sidebar.newFolder")} onClick={() => void app.createNode("folder", null)} />
          </div>
          <ErrorBanner error={tree.error()}>
            <button type="button" class="link-button" onClick={() => void tree.refresh()}>
              {t("common.retry")}
            </button>
          </ErrorBanner>
          <Show when={tree.loaded()} fallback={<Spinner />}>
            <TreeView />
          </Show>
        </section>
      </div>

      <div class="sidebar-footer">
        <NavButton icon="trash" label={t("sidebar.trash")} target={{ kind: "trash" }} />
        <NavButton icon="key" label={t("sidebar.vault")} target={{ kind: "vault" }} />
        <NavButton icon="settings" label={t("sidebar.settings")} target={{ kind: "settings" }} />
      </div>

      <Show when={menuAt()}>{(at) => <Menu label={t("sidebar.workspaceMenu")} at={at()} entries={workspaceEntries()} onClose={() => setMenuAt(null)} />}</Show>
    </nav>
  );
}
