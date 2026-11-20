import { Match, Show, Switch, onCleanup, onMount, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { isMac } from "../../lib/device.ts";
import { useApp } from "../../state/app.tsx";
import { SIDEBAR_DEFAULT, SIDEBAR_MAX, SIDEBAR_MIN } from "../../state/ui-store.ts";
import { PageView } from "../editor/page-view.tsx";
import { SearchPalette } from "../search/search-palette.tsx";
import { SettingsPage } from "../settings/settings-page.tsx";
import { ToastHost } from "../common/toast-host.tsx";
import { IconButton } from "../common/button.tsx";
import { ReauthDialog } from "../vault/vault-dialogs.tsx";
import { VaultView } from "../vault/vault-view.tsx";
import { RecordingIndicator, TalkButton } from "../voice/voice-controls.tsx";
import { Breadcrumb } from "./breadcrumb.tsx";
import { HomeView } from "./home-view.tsx";
import { Sidebar } from "./sidebar.tsx";
import { SyncIndicator } from "./sync-indicator.tsx";
import { TabBar } from "./tab-bar.tsx";
import { ThemeSwitch } from "./theme-switch.tsx";
import { TrashView } from "./trash-view.tsx";

/** Drag handle between sidebar and content; arrow keys resize it too. */
function Resizer(): JSX.Element {
  const { ui } = useApp();
  const onPointerDown = (event: PointerEvent) => {
    event.preventDefault();
    const startX = event.clientX;
    const startWidth = ui.sidebarWidth();
    const move = (moveEvent: PointerEvent) => ui.setSidebarWidth(startWidth + moveEvent.clientX - startX);
    const stop = () => {
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", stop);
    };
    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", stop);
  };
  return (
    <div
      class="resizer"
      role="separator"
      aria-orientation="vertical"
      aria-label={t("shell.resizeSidebar")}
      aria-valuemin={SIDEBAR_MIN}
      aria-valuemax={SIDEBAR_MAX}
      aria-valuenow={ui.sidebarWidth()}
      tabindex="0"
      onPointerDown={onPointerDown}
      onDblClick={() => ui.setSidebarWidth(SIDEBAR_DEFAULT)}
      onKeyDown={(event) => {
        if (event.key === "ArrowLeft") (event.preventDefault(), ui.setSidebarWidth(ui.sidebarWidth() - 16));
        else if (event.key === "ArrowRight") (event.preventDefault(), ui.setSidebarWidth(ui.sidebarWidth() + 16));
        else if (event.key === "Home") (event.preventDefault(), ui.setSidebarWidth(SIDEBAR_MIN));
        else if (event.key === "End") (event.preventDefault(), ui.setSidebarWidth(SIDEBAR_MAX));
      }}
    />
  );
}

function TopBar(): JSX.Element {
  const { tabs, ui } = useApp();
  return (
    <header class="topbar">
      <IconButton icon="sidebar" label={t("shell.toggleSidebar")} pressed={!ui.sidebarHidden()} onClick={() => ui.toggleSidebar()} />
      <IconButton icon="arrow-left" label={t("shell.back")} disabled={!tabs.canBack()} onClick={() => tabs.back()} />
      <IconButton icon="arrow-right" label={t("shell.forward")} disabled={!tabs.canForward()} onClick={() => tabs.forward()} />
      <Breadcrumb />
      <div class="topbar-spacer" />
      <RecordingIndicator />
      <TalkButton />
      <SyncIndicator />
      <ThemeSwitch />
    </header>
  );
}

/** Keyboard shortcuts that work anywhere in the workspace. */
function useGlobalShortcuts(): void {
  const app = useApp();
  onMount(() => {
    const onKeyDown = (event: KeyboardEvent) => {
      if (event.key === "Escape" && app.voice.recording()) {
        event.preventDefault();
        void app.voice.cancel();
        return;
      }
      const mod = isMac() ? event.metaKey : event.ctrlKey;
      if (!mod || event.altKey) return;
      const key = event.key.toLowerCase();
      if (key === "k" && !event.shiftKey) {
        event.preventDefault();
        app.ui.setSearchOpen(!app.ui.searchOpen());
      } else if (key === "f" && !event.shiftKey && app.tabs.activePageId()) {
        event.preventDefault();
        app.ui.requestFind();
      } else if (key === "[" || key === "]") {
        event.preventDefault();
        if (key === "[") app.tabs.back();
        else app.tabs.forward();
      } else if (key === "t" && !event.shiftKey) {
        event.preventDefault();
        app.tabs.newTab();
      } else if (key === "w" && !event.shiftKey) {
        event.preventDefault();
        app.tabs.closeActive();
      } else if (key === "\\") {
        event.preventDefault();
        app.ui.toggleSidebar();
      }
    };
    window.addEventListener("keydown", onKeyDown);
    onCleanup(() => window.removeEventListener("keydown", onKeyDown));
  });
}

/** The signed-in app: sidebar, tabs, and whatever the active tab shows. */
export function Workspace(): JSX.Element {
  const app = useApp();
  useGlobalShortcuts();
  const pageId = () => app.tabs.activePageId();
  return (
    <div class="workspace" style={{ "--sidebar-w": `${app.ui.sidebarWidth()}px` }}>
      <Show when={!app.ui.sidebarHidden()}>
        <Sidebar />
        <Resizer />
      </Show>
      <div class="main">
        <TopBar />
        <TabBar />
        <main class="content" id="content" tabindex="-1">
          <Switch fallback={<HomeView />}>
            <Match when={pageId()}>
              {/* keyed: each page gets its own editor instance */}
              <Show when={pageId()} keyed>
                {(id) => <PageView pageId={id} />}
              </Show>
            </Match>
            <Match when={app.tabs.target().kind === "trash"}>
              <TrashView />
            </Match>
            <Match when={app.tabs.target().kind === "vault"}>
              <VaultView />
            </Match>
            <Match when={app.tabs.target().kind === "settings"}>
              <SettingsPage />
            </Match>
          </Switch>
        </main>
      </div>
      <Show when={app.ui.searchOpen()}>
        <SearchPalette onClose={() => app.ui.setSearchOpen(false)} />
      </Show>
      <ReauthDialog />
      <ToastHost />
    </div>
  );
}
