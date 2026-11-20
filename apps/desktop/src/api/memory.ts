import type { LoroDoc } from "loro-crdt";
import type { CellValue, CoreApi, CoreEvents, NodeId } from "./contract.ts";
import { createAccount, type AccountControl, type AccountHooks } from "./memory-account.ts";
import { createBus, type MemoryOptions } from "./memory-bus.ts";
import { createDbStore } from "./memory-db.ts";
import { createHistory } from "./memory-history.ts";
import { createStructuredImport } from "./memory-import.ts";
import { createReminders } from "./memory-reminders.ts";
import { createSynced } from "./memory-synced.ts";
import { createUpdates, type UpdatesControl } from "./memory-updates.ts";
import { createTemplates } from "./memory-templates.ts";
import { createTreeStore } from "./memory-tree.ts";
import { createVault, type VaultControl } from "./memory-vault.ts";
import { createWorkspace } from "./memory-workspace.ts";

export { MEMORY_SETUP_CODE, MIN_PASSWORD_LENGTH } from "./memory-bus.ts";
export type { MemoryOptions } from "./memory-bus.ts";

export interface MemoryControl extends AccountControl, VaultControl, UpdatesControl {
  /** Merge an update as if another device sent it, and notify the open editor. */
  receiveRemoteUpdate(pageId: NodeId, update: Uint8Array): void;
  /** The core's copy of a page document. */
  pageDoc(pageId: NodeId): LoroDoc | undefined;
  /** Edit a cell as if another device did: the UI hears of it through `tree:changed` only. */
  receiveRemoteCell(databaseId: NodeId, rowId: NodeId, propertyId: string, value: CellValue | null): Promise<void>;
  /** Merge an update to a shared document as if another device sent it. */
  receiveRemoteSyncedUpdate(syncId: string, update: Uint8Array): void;
  /** The core's copy of a shared document. */
  syncedDoc(syncId: string): LoroDoc | undefined;
  /** Mark a page's history as trimmed, as if it arrived as a trimmed copy. */
  setHistoryTrimmed(pageId: NodeId, trimmed: boolean): void;
  /** Move the clock the reminders use forward, then fire whatever is due. */
  advanceClock(ms: number): void;
  /** Fire whatever reminders are due now. */
  checkReminders(): void;
  /** Make rows quickly, for tests that need thousands; no events are emitted per row. */
  seedRows(databaseId: NodeId, count: number, make?: (index: number) => { title: string; values?: Record<string, CellValue> }): Promise<NodeId[]>;
}

export interface MemoryCore {
  api: CoreApi;
  events: CoreEvents;
  control: MemoryControl;
}

/** A few bytes standing in for a recording; nothing is ever played from it. */
const FAKE_RECORDING = new Uint8Array([82, 73, 70, 70, 0, 0, 0, 0, 87, 65, 86, 69]);

/**
 * In-memory implementation of the UI to core contract. It is used by tests and
 * by the browser preview (`pnpm dev` outside Tauri) and is never part of a
 * production bundle: `api/index.ts` only imports it in development.
 */
export function createMemoryCore(options: MemoryOptions = {}): MemoryCore {
  const bus = createBus();
  let skew = 0;
  const now = () => (options.now ?? Date.now)() + skew;
  const accountHooks: AccountHooks = {};
  const account = createAccount(bus, options, accountHooks);
  const tree = createTreeStore({
    created: (node) => workspace.createPageDoc(node),
    deleted: (id) => workspace.dropPageDoc(id),
    changed: (nodeIds) => {
      account.markDirty();
      bus.emit("tree:changed", { nodeIds });
    },
  });
  const workspace = createWorkspace(tree, bus, options, account.markDirty);
  const vault = createVault(bus, options, () => account.settings().vaultIdleLockSeconds, account.accountPassword);
  const db = createDbStore(tree, bus, account.markDirty);
  const history = createHistory({ doc: workspace.doc, publish: workspace.publishChange, isLive: tree.isLive });
  const synced = createSynced({
    doc: workspace.doc,
    allDocs: workspace.allDocs,
    publish: workspace.publishChange,
    title: (id) => tree.get(id)?.title ?? "",
    isLive: tree.isLive,
    publishShared: (syncId, update) => bus.emit("page:remote-update", { pageId: syncId, update }),
    now,
  });
  const templates = createTemplates({ tree, db, doc: workspace.doc, replaceDoc: workspace.replaceDoc, now, markDirty: account.markDirty });
  const updates = createUpdates(bus, now, options.updatesUnavailable);
  const reminders = createReminders(tree, db, bus, now, options.nativeReminders ?? false);
  const structured = createStructuredImport({
    tree,
    db,
    doc: workspace.doc,
    pickFiles: workspace.pickFiles,
    addAttachment: workspace.addAttachment,
    markDirty: account.markDirty,
  });
  accountHooks.recordAudio = () => workspace.addAttachment(FAKE_RECORDING, "audio/wav");

  const { control: accountControl, markDirty: _markDirty, accountPassword: _pw, settings: _settings, ...accountApi } = account;
  const {
    createPageDoc: _c,
    dropPageDoc: _d,
    receiveRemoteUpdate,
    doc,
    allDocs: _a,
    replaceDoc: _r,
    publishChange: _p,
    addAttachment: _aa,
    pickFiles: _pf,
    ...workspaceApi
  } = workspace;
  const { get: _g, breadcrumb: _b, livePages: _l, isLive: _i, touch: _t, liveChildren: _lc, ...treeApi } = tree;
  const { rowCells: _rc, setRowCells: _sc, receiveRemoteCell, schemaOf: _so, addProperty: _ap, ...dbApi } = db;
  const { setTrimmed, ...historyApi } = history;
  const { receiveRemoteUpdate: receiveRemoteSynced, sharedDoc, ...syncedApi } = synced;
  const { check: checkReminders, ...reminderApi } = reminders;
  const { control: updatesControl, ...updatesApi } = updates;

  return {
    api: { ...accountApi, ...treeApi, ...workspaceApi, ...vault.api, ...dbApi, ...templates, ...historyApi, ...structured, ...syncedApi, ...reminderApi, ...updatesApi },
    events: bus.events,
    control: {
      ...accountControl,
      ...vault.control,
      ...updatesControl,
      receiveRemoteUpdate,
      pageDoc: doc,
      receiveRemoteCell,
      receiveRemoteSyncedUpdate: receiveRemoteSynced,
      syncedDoc: sharedDoc,
      setHistoryTrimmed: setTrimmed,
      advanceClock(ms) {
        skew += ms;
        checkReminders();
      },
      checkReminders,
      async seedRows(databaseId, count, make) {
        const ids: NodeId[] = [];
        for (let at = 0; at < count; at += 1) {
          const row = make?.(at) ?? { title: `Row ${at + 1}` };
          const id = await tree.treeCreate({ kind: "page", parentId: databaseId, title: row.title });
          if (row.values) db.setRowCells(databaseId, id, row.values);
          ids.push(id);
        }
        return ids;
      },
    },
  };
}
