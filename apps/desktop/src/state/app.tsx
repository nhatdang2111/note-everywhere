import { createContext, createEffect, on, onCleanup, onMount, useContext, type JSX } from "solid-js";
import type { Core } from "../api/index.ts";
import type { NodeId, NodeKind, SearchHit, StructuredImportReport } from "../api/contract.ts";
import { toApiError } from "../api/errors.ts";
import type { PageLinkInfo } from "../editor/index.ts";
import { t } from "../i18n/index.ts";
import { showNativeNotification } from "../lib/notify.ts";
import { createRecents, type Recents } from "./recents.ts";
import { createRemindersStore, type RemindersStore } from "./reminders-store.ts";
import { createRowRegistry, type RowRegistry } from "./rows.ts";
import { createSyncedHub, type SyncedHub } from "./synced-hub.ts";
import { createSessionStore, type SessionStore } from "./session-store.ts";
import { createSettingsStore, type SettingsStore } from "./settings-store.ts";
import { createUpdateStore, type UpdateStore } from "./update-store.ts";
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
  /** Rows of databases that are open or were opened: they are not in the tree. */
  rows: RowRegistry;
  /** Shared content of synced blocks. */
  synced: SyncedHub;
  reminders: RemindersStore;
  updates: UpdateStore;
  /** Title of a page, folder, database or known row. */
  pageTitle(id: NodeId): string;
  /** Whether a page (or row, given its database) can be opened. */
  pageState(id: NodeId, databaseId?: NodeId): "ok" | "trashed" | "missing";
  /** Open a row of a database as a page. */
  openRow(databaseId: NodeId, rowId: NodeId, title: string, inNewTab?: boolean): void;
  /** Open what a search found; a row is traced back to its database when the tree makes that certain. */
  openSearchHit(hit: SearchHit, inNewTab?: boolean): void;
  renamePage(id: NodeId, title: string): Promise<void>;
  importStructured(parentId: NodeId | null): Promise<StructuredImportReport | null>;
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
  const rows = createRowRegistry();
  const synced = createSyncedHub(core, toasts.error);
  const reminders = createRemindersStore(core);
  const updates = createUpdateStore(core, toasts.error);
  const voice = createVoiceStore(core, ui, {
    onError: toasts.error,
    onOrphanText: (text) => void toasts.push("info", t("voice.noPageOpen", { text })),
  });
  onCleanup(() => {
    vault.dispose();
    synced.dispose();
  });

  const guard = async <T,>(run: () => Promise<T>): Promise<T | undefined> => {
    try {
      return await run();
    } catch (error) {
      if (toApiError(error).code !== "cancelled") toasts.error(error);
      return undefined;
    }
  };

  const pageTitle = (id: NodeId): string => tree.title(id) || rows.title(id) || "";

  const pageState = (id: NodeId, databaseId?: NodeId): "ok" | "trashed" | "missing" => {
    const state = tree.pageState(id);
    if (state !== "missing") return state;
    const owner = databaseId ?? rows.databaseOf(id);
    return owner ? tree.pageState(owner) : "missing";
  };

  const openPage = (id: NodeId, inNewTab = false) => {
    const databaseId = tree.index().byId.has(id) ? undefined : rows.databaseOf(id);
    tabs.open({ kind: "page", pageId: id, ...(databaseId ? { databaseId } : {}) }, { newTab: inNewTab });
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
    rows,
    synced,
    reminders,
    updates,
    guard,
    pageTitle,
    pageState,
    openPage,
    openRow(databaseId, rowId, title, inNewTab = false) {
      rows.remember(rowId, databaseId, title);
      tabs.open({ kind: "page", pageId: rowId, databaseId }, { newTab: inNewTab });
    },
    openSearchHit(hit, inNewTab = false) {
      if (!tree.index().byId.has(hit.pageId) && !rows.databaseOf(hit.pageId)) {
        // A row: its breadcrumb ends with the database's title, which names the database when exactly one fits.
        const owners = tree.nodes().filter((node) => node.kind === "database" && node.title === hit.breadcrumb.at(-1));
        if (owners.length === 1 && owners[0]) rows.remember(hit.pageId, owners[0].id, hit.title);
      }
      openPage(hit.pageId, inNewTab);
    },
    async renamePage(id, title) {
      const databaseId = tree.index().byId.has(id) ? undefined : rows.databaseOf(id);
      if (!databaseId) return tree.rename(id, title);
      await core.api.dbCellSet({ id: databaseId, rowId: id, propertyId: "title", value: { type: "text", value: title } });
      rows.retitle(id, title);
    },
    async importStructured(parentId) {
      const report = await guard(() => core.api.importStructured({ parentId }));
      if (report) await tree.refresh();
      return report ?? null;
    },
    openTarget: (target) => tabs.open(target),
    pageInfo(id) {
      const state = pageState(id);
      const title = state === "trashed" ? (tree.trash().find((entry) => entry.id === id)?.title ?? "") : pageTitle(id);
      return { state, title };
    },
    async createNode(kind, parentId) {
      const id = await guard(async () => {
        if (kind !== "database") return tree.create({ kind, parentId });
        const created = await core.api.dbCreate({ parentId });
        if (parentId) tree.setExpanded(parentId, true);
        await tree.refresh();
        return created;
      });
      if (id === undefined) return null;
      if (kind === "page" || kind === "database") openPage(id);
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
        void reminders.refresh();
        void updates.refresh();
      },
    ),
  );

  // Tabs never keep pointing at pages that are gone.
  createEffect(() => {
    if (!tree.loaded() || tree.error()) return;
    const index = tree.index();
    tabs.prune((pageId, databaseId) => (databaseId ? index.byId.has(databaseId) : index.byId.has(pageId) || rows.databaseOf(pageId) !== undefined));
  });

  onMount(() => {
    void session.refresh();
    let unlisten: (() => void) | undefined;
    let disposed = false;
    void core.events
      .on("voice:hotkey", ({ pressed }) => void (pressed ? voice.press() : voice.release()))
      .then((stop) => (disposed ? stop() : (unlisten = stop)));
    let stopDue: (() => void) | undefined;
    void core.events
      .on("reminder:due", ({ reminders: due, nativeShown }) => {
        const first = due[0];
        // The core already showed the system notification: a second message here would duplicate it.
        if (!first || nativeShown) return;
        const title = first.pageTitle || t("tree.untitled");
        const message = due.length === 1 ? t("reminder.due", { title }) : t("reminder.dueMany", { title, count: due.length - 1 });
        const open = first.pageVisible ? { label: t("reminder.open"), run: () => openPage(first.reminder.pageId) } : undefined;
        toasts.push("info", first.reminder.note && due.length === 1 ? `${message} ${first.reminder.note}` : message, 12000, open);
        showNativeNotification(t("app.name"), message);
      })
      .then((stop) => (disposed ? stop() : (stopDue = stop)));
    const warn = (event: BeforeUnloadEvent) => {
      if (session.pendingRecoveryKey() || vault.pendingRecoveryKey()) event.preventDefault();
    };
    window.addEventListener("beforeunload", warn);
    onCleanup(() => {
      disposed = true;
      unlisten?.();
      stopDue?.();
      window.removeEventListener("beforeunload", warn);
    });
  });

  return app;
}

export function AppProvider(props: { core: Core; children: JSX.Element }): JSX.Element {
  const app = createApp(props.core);
  return <AppContext.Provider value={app}>{props.children}</AppContext.Provider>;
}
