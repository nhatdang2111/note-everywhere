import { For, type JSX } from "solid-js";
import { t, type MessageKey } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { currentTarget } from "../../state/tabs-model.ts";
import { IconButton } from "../common/button.tsx";
import { Icon, type IconName } from "../common/icons.tsx";

const LABELS: Record<string, MessageKey> = { home: "tabs.home", trash: "tabs.trash", vault: "tabs.vault", settings: "tabs.settings" };
const ICONS: Record<string, IconName> = { home: "page", trash: "trash", vault: "key", settings: "settings", page: "page" };

export function TabBar(): JSX.Element {
  const app = useApp();
  const { tabs } = app;

  const label = (tab: Parameters<typeof currentTarget>[0]) => {
    const target = currentTarget(tab);
    if (target.kind === "page") return app.pageTitle(target.pageId) || t("tree.untitled");
    return t(LABELS[target.kind] ?? "tabs.home");
  };

  const onKeyDown = (event: KeyboardEvent) => {
    const list = tabs.tabs();
    const at = list.findIndex((tab) => tab.id === tabs.activeId());
    const go = (to: number) => {
      event.preventDefault();
      const next = list[(to + list.length) % list.length];
      if (next) tabs.activate(next.id);
    };
    if (event.key === "ArrowRight") go(at + 1);
    else if (event.key === "ArrowLeft") go(at - 1);
    else if (event.key === "Home") go(0);
    else if (event.key === "End") go(list.length - 1);
  };

  return (
    <div class="tab-bar">
      <div class="tabs" role="tablist" aria-label={t("tabs.label")} onKeyDown={onKeyDown}>
        <For each={tabs.tabs()}>
          {(tab) => {
            const selected = () => tabs.activeId() === tab.id;
            return (
              <div class="tab" data-selected={selected() ? "true" : "false"}>
                <button
                  type="button"
                  role="tab"
                  class="tab-label"
                  aria-selected={selected()}
                  tabindex={selected() ? 0 : -1}
                  onClick={() => tabs.activate(tab.id)}
                  onAuxClick={(event) => event.button === 1 && tabs.close(tab.id)}
                >
                  <Icon name={ICONS[currentTarget(tab).kind] ?? "page"} size={14} />
                  <span class="tab-title">{label(tab)}</span>
                </button>
                <IconButton icon="close" size={12} label={t("tabs.close", { title: label(tab) })} tabindex={-1} onClick={() => tabs.close(tab.id)} />
              </div>
            );
          }}
        </For>
      </div>
      <IconButton icon="plus" label={t("tabs.new")} onClick={() => tabs.newTab()} />
    </div>
  );
}
