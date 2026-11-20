import { LoroDoc } from "loro-crdt";
import type { CoreApi, NodeId, SyncedBlockInfo, SyncedOccurrence } from "./contract.ts";
import { apiError } from "./errors.ts";
import { randomId } from "./memory-bus.ts";
import {
  childList,
  containsSynced,
  copyChildren,
  copyChildrenAt,
  createSharedDoc,
  ensureNotEmpty,
  insertNode,
  nodeAtPath,
  rootChildren,
  syncedRefs,
  attrsOf,
} from "./memory-doc-ops.ts";

type SyncedMethods = Pick<
  CoreApi,
  | "syncedCreate"
  | "syncedInsert"
  | "syncedOpen"
  | "syncedApplyUpdate"
  | "syncedClose"
  | "syncedUnsync"
  | "syncedOccurrences"
  | "syncedList"
>;

export interface Synced extends SyncedMethods {
  /** An update to a shared document that "arrived from another device". */
  receiveRemoteUpdate(syncId: string, update: Uint8Array): void;
  sharedDoc(syncId: string): LoroDoc | undefined;
}

interface Hooks {
  doc(id: NodeId): LoroDoc | undefined;
  allDocs(): ReadonlyMap<NodeId, LoroDoc>;
  publish(pageId: NodeId, update: Uint8Array): void;
  title(id: NodeId): string;
  isLive(id: NodeId): boolean;
  /** Tell open editors of a shared document about a change that did not come from them. */
  publishShared(syncId: string, update: Uint8Array): void;
  now(): number;
}

interface Shared {
  doc: LoroDoc;
  orphanedAt: number | null;
}

export function createSynced(hooks: Hooks): Synced {
  const shared = new Map<string, Shared>();

  const needShared = (syncId: string): Shared => {
    const found = shared.get(syncId);
    if (!found) throw apiError("not_found", "That shared content has not arrived on this device yet.");
    return found;
  };

  const needPage = (pageId: NodeId): LoroDoc => {
    const doc = hooks.doc(pageId);
    if (!doc || !hooks.isLive(pageId)) throw apiError("not_found", "That page is not available.");
    return doc;
  };

  /** Run `change` on a page document and hand the resulting update to the open editor. */
  function edit(pageId: NodeId, change: (doc: LoroDoc) => void): void {
    const doc = needPage(pageId);
    const before = doc.version();
    change(doc);
    ensureNotEmpty(doc);
    doc.commit();
    hooks.publish(pageId, doc.export({ mode: "update", from: before }));
  }

  const occurrences = (syncId: string): SyncedOccurrence[] => {
    const found: SyncedOccurrence[] = [];
    for (const [pageId, doc] of hooks.allDocs()) {
      if (syncedRefs(doc).includes(syncId)) found.push({ pageId, title: hooks.title(pageId), trashed: !hooks.isLive(pageId) });
    }
    return found;
  };

  return {
    sharedDoc: (syncId) => shared.get(syncId)?.doc,
    receiveRemoteUpdate(syncId, update) {
      needShared(syncId).doc.import(update);
      hooks.publishShared(syncId, update);
    },

    async syncedCreate({ pageId, start, end }) {
      const page = needPage(pageId);
      const list = rootChildren(page);
      if (!Number.isInteger(start) || !Number.isInteger(end) || start < 0 || end < start || end >= list.length) {
        throw apiError("invalid_input", "Choose existing top-level blocks.");
      }
      if (containsSynced(list, start, end)) throw apiError("invalid_input", "A synced block cannot contain another synced block.");
      const syncId = `${shared.size}@${randomId("peer").slice(5)}`;
      const doc = createSharedDoc();
      copyChildren(list, rootChildren(doc), start, end);
      doc.commit();
      shared.set(syncId, { doc, orphanedAt: null });
      edit(pageId, (target) => {
        const blocks = rootChildren(target);
        blocks.delete(start, end - start + 1);
        insertNode(blocks, start, "synced_block", { syncId });
      });
      return { syncId };
    },

    async syncedInsert({ pageId, parentPath, index, syncId }) {
      needShared(syncId);
      const page = needPage(pageId);
      const parent = nodeAtPath(page, parentPath);
      const list = parent ? childList(parent) : undefined;
      const name = parent ? String(parent.get("nodeName") ?? "") : "";
      const holdsBlocks = ["doc", "quote", "callout", "toggle", "list_item", "todo_item"].includes(name);
      if (!list || !holdsBlocks || index < 0 || index > list.length) throw apiError("invalid_input", "A synced block cannot go there.");
      edit(pageId, (doc) => {
        const target = nodeAtPath(doc, parentPath);
        const children = target ? childList(target) : undefined;
        if (children) insertNode(children, index, "synced_block", { syncId });
      });
      const entry = shared.get(syncId);
      if (entry) entry.orphanedAt = null;
    },

    async syncedOpen({ syncId }) {
      return { snapshot: needShared(syncId).doc.export({ mode: "snapshot" }) };
    },
    async syncedApplyUpdate({ syncId, update }) {
      try {
        needShared(syncId).doc.import(update);
      } catch (error) {
        if ((error as { code?: string }).code === "not_found") throw error;
        throw apiError("bad_update", "The update could not be applied.");
      }
    },
    async syncedClose() {},

    async syncedUnsync({ pageId, path }) {
      const page = needPage(pageId);
      const node = nodeAtPath(page, path);
      const syncId = node && String(node.get("nodeName")) === "synced_block" ? String(attrsOf(node).syncId ?? "") : "";
      if (!syncId) throw apiError("invalid_input", "There is no synced block there.");
      const entry = needShared(syncId);
      edit(pageId, (doc) => {
        const parentPath = path.slice(0, -1);
        const at = path[path.length - 1] as number;
        const parent = nodeAtPath(doc, parentPath);
        const children = parent ? childList(parent) : undefined;
        if (!children) return;
        children.delete(at, 1);
        copyChildrenAt(rootChildren(entry.doc), children, at);
      });
      if (occurrences(syncId).length === 0) shared.delete(syncId);
    },

    async syncedOccurrences({ syncId }) {
      needShared(syncId);
      return occurrences(syncId);
    },
    async syncedList(): Promise<SyncedBlockInfo[]> {
      return [...shared].map(([syncId, entry]) => {
        const count = occurrences(syncId).length;
        if (count > 0) entry.orphanedAt = null;
        else entry.orphanedAt ??= hooks.now();
        return { syncId, occurrences: count, orphanedAt: entry.orphanedAt };
      });
    },
  };
}
