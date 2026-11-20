import { For, Show, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { modLabel } from "../../lib/device.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Icon } from "../common/icons.tsx";

/** What a new tab shows: recent pages and a way to start writing. */
export function HomeView(): JSX.Element {
  const app = useApp();
  const recent = () => app.recents.ids().filter((id) => app.tree.index().byId.has(id));
  return (
    <section class="page-surface home" aria-labelledby="home-title">
      <h1 id="home-title">{t("home.title")}</h1>
      <p class="view-intro">{t("home.intro", { mod: modLabel() })}</p>
      <div class="home-actions">
        <Button variant="primary" icon="page" onClick={() => void app.createNode("page", null)}>
          {t("sidebar.newPage")}
        </Button>
        <Button icon="search" onClick={() => app.ui.setSearchOpen(true)}>
          {t("sidebar.search")}
        </Button>
      </div>
      <Show when={recent().length > 0}>
        <h2 class="section-title">{t("home.recent")}</h2>
        <ul class="recent-list">
          <For each={recent()}>
            {(id) => (
              <li>
                <button type="button" class="nav-button" onClick={() => app.openPage(id)}>
                  <Icon name="page" />
                  <span class="nav-label">{app.tree.title(id) || t("tree.untitled")}</span>
                </button>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </section>
  );
}
