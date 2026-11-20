import type { LoroDoc } from "loro-crdt";
import type { CoreApi, CoreEvents, NodeId } from "./contract.ts";
import { createAccount, type AccountControl } from "./memory-account.ts";
import { createBus, type MemoryOptions } from "./memory-bus.ts";
import { createTreeStore } from "./memory-tree.ts";
import { createVault, type VaultControl } from "./memory-vault.ts";
import { createWorkspace } from "./memory-workspace.ts";

export { MEMORY_SETUP_CODE, MIN_PASSWORD_LENGTH } from "./memory-bus.ts";
export type { MemoryOptions } from "./memory-bus.ts";

export interface MemoryControl extends AccountControl, VaultControl {
  /** Merge an update as if another device sent it, and notify the open editor. */
  receiveRemoteUpdate(pageId: NodeId, update: Uint8Array): void;
  /** The core's copy of a page document. */
  pageDoc(pageId: NodeId): LoroDoc | undefined;
}

export interface MemoryCore {
  api: CoreApi;
  events: CoreEvents;
  control: MemoryControl;
}

/**
 * In-memory implementation of the UI to core contract. It is used by tests and
 * by the browser preview (`pnpm dev` outside Tauri) and is never part of a
 * production bundle: `api/index.ts` only imports it in development.
 */
export function createMemoryCore(options: MemoryOptions = {}): MemoryCore {
  const bus = createBus();
  const account = createAccount(bus, options);
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

  const { control: accountControl, markDirty: _markDirty, accountPassword: _pw, settings: _settings, ...accountApi } = account;
  const { createPageDoc: _c, dropPageDoc: _d, receiveRemoteUpdate, doc, ...workspaceApi } = workspace;
  const { get: _g, breadcrumb: _b, livePages: _l, isLive: _i, touch: _t, ...treeApi } = tree;

  return {
    api: { ...accountApi, ...treeApi, ...workspaceApi, ...vault.api },
    events: bus.events,
    control: { ...accountControl, ...vault.control, receiveRemoteUpdate, pageDoc: doc },
  };
}
