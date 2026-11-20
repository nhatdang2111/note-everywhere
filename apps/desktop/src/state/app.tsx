import { createContext, createEffect, on, onCleanup, onMount, useContext, type JSX } from "solid-js";
import type { Core } from "../api/index.ts";
import type { NodeId, NodeKind } from "../api/contract.ts";
import { toApiError } from "../api/errors.ts";
import type { PageLinkInfo } from "../editor/index.ts";
import { t } from "../i18n/index.ts";
import { createRecents, type Recents } from "./recents.ts";
import { createSessionStore, type SessionStore } from "./session-store.ts";
import { createSettingsStore, type SettingsStore } from "./settings-store.ts";
import { createSyncStore, type SyncStore } from "./sync-store.ts";
import { createTabsStore, type TabsStore } from "./tabs-store.ts";
import type { Target } from "./tabs-model.ts";
import { createToasts, type Toasts } from "./toasts.ts";
import { createTreeStore, type TreeStore } from "./tree-store.ts";
import { createUiStore, type UiStore } from "./ui-store.ts";
import { createVaultStore, type VaultStore } from "./vault-store.ts";
import { createVoiceStore, type VoiceStore } from "./voice-store.ts";

export interface App {
  core: Core;
  toasts: Toasts;
  session: SessionStore;
  tree: TreeStore;
  tabs: TabsStore;
  ui: UiStore;
  settings: SettingsStore;
  sync: SyncStore;
  vault: VaultStore;
  voice: VoiceStore;
  recents: Recents;
  /** Live title and state of a page, for links inside documents. */
  pageInfo(id: NodeId): PageLinkInfo | null;
  openPage(id: NodeId, inNewTab?: boolean): void;
  openTarget(target: Target): void;
  /** Create a page or folder; pages open at once, folders start renaming. */
  createNode(kind: NodeKind, parentId: NodeId | null): Promise<NodeId | null>;
  trashNode(id: NodeId): Promise<void>;
  exportMarkdown(id: NodeId | null): Promise<void>;
  importMarkdown(parentId: NodeId | null): Promise<void>;
  /** Run an async action and show its `ApiError.message` on failure. */
  guard<T>(run: () => Promise<T>): Promise<T | undefined>;
}

const AppContext = createContext<App>();

export function useApp(): App {
  const app = useContext(AppContext);
  if (!app) throw new Error("useApp must be used inside <AppProvider>");
  return app;
}

/** Build every store for a core. Call inside a component so effects have an owner. */
export function createApp(core: Core): App {
  const toasts = createToasts();
  const ui = createUiStore();
  const session = createSessionStore(core);
  const tree = createTreeStore(core);
  const tabs = createTabsStore();
  const settings = createSettingsStore(core, toasts.error);
  const sync = createSyncStore(core, toasts.error);
  const vault = createVaultStore(core);
  const recents = createRecents();
  const voice = createVoiceStore(core, ui, {
    onError: toasts.error,
    onOrphanText: (text) => void toasts.push("info", t("voice.noPageOpen", { text })),
  });
  onCleanup(() => vault.dispose());

  const guard = async <T,>(run: () => Promise<T>): Promise<T | undefined> => {
    try {
      return await run();
    } catch (error) {
      if (toApiError(error).code !== "cancelled") toasts.error(error);
      return undefined;
    }
  };

  const openPage = (id: NodeId, inNewTab = false) => {
    tabs.open({ kind: "page", pageId: id }, { newTab: inNewTab });
    recents.touch(id);
  };

  const app: App = {
    core,
    toasts,
    session,
    tree,
    tabs,
    ui,
    settings,
    sync,
    vault,
    voice,
    recents,
    guard,
    openPage,
    openTarget: (target) => tabs.open(target),
    pageInfo(id) {
      const state = tree.pageState(id);
      const title = state === "trashed" ? (tree.trash().find((entry) => entry.id === id)?.title ?? "") : tree.title(id);
      return { state, title };
    },
    async createNode(kind, parentId) {
      const id = await guard(() => tree.create({ kind, parentId }));
      if (id === undefined) return null;
      if (kind === "page") openPage(id);
      else ui.setRenamingId(id);
      return id;
    },
    async trashNode(id) {
      const title = tree.title(id) || t("tree.untitled");
      try {
        await tree.moveToTrash(id);
        toasts.push("info", t("tree.movedToTrash", { title }));
      } catch (error) {
        toasts.error(error);
      }
    },
    async exportMarkdown(id) {
      const result = await guard(() => core.api.exportMarkdown({ id }));
      if (result) toasts.push("success", t("export.done", { count: result.pagesWritten }));
    },
    async importMarkdown(parentId) {
      const result = await guard(() => core.api.importMarkdown({ parentId }));
      if (!result) return;
      await tree.refresh();
      toasts.push("success", t("import.done", { count: result.pagesCreated }));
    },
  };

  // Load everything that needs a workspace once there is one.
  createEffect(
    on(
      () => session.status()?.state,
      (state) => {
        if (state !== "local" && state !== "signed_in") return;
        void tree.refresh();
        void settings.refresh();
        void sync.refresh();
        void vault.refreshState();
        void voice.refresh();
      },
    ),
  );

  // Tabs never keep pointing at pages that are gone.
  createEffect(() => {
    if (!tree.loaded() || tree.error()) return;
    const index = tree.index();
    tabs.prune((pageId) => index.byId.has(pageId));
  });

  onMount(() => {
    void session.refresh();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void core.events
      .on("voice:hotkey", ({ pressed }) => void (pressed ? voice.press() : voice.release()))
      .then((stop) => (disposed ? stop() : (unlisten = stop)));
    const warn = (event: BeforeUnloadEvent) => {
      if (session.pendingRecoveryKey() || vault.pendingRecoveryKey()) event.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    onCleanup(() => {
      disposed = true;
      unlisten?.();
      window.removeEventListener("beforeunload", warn);
    });
  });

  return app;
}

export function AppProvider(props: { core: Core; children: JSX.Element }): JSX.Element {
  const app = createApp(props.core);
  return <AppContext.Provider value={app}>{props.children}</AppContext.Provider>;
}
