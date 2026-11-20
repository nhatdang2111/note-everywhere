import type { CoreApi, NodeId, TrashEntry, TreeNode } from "./contract.ts";
import { apiError } from "./errors.ts";
import { randomId } from "./memory-bus.ts";

type TreeMethods = Pick<
  CoreApi,
  | "treeSnapshot"
  | "treeCreate"
  | "treeRename"
  | "treeSetIcon"
  | "treeSetFavorite"
  | "treeMove"
  | "treeTrash"
  | "treeRestore"
  | "treeDeletePermanently"
  | "treeListTrash"
  | "treeEmptyTrash"
>;

export interface TreeStore extends TreeMethods {
  get(id: NodeId): TreeNode | undefined;
  /** Titles of the ancestors of `id`, outermost first. */
  breadcrumb(id: NodeId): string[];
  /** Pages that are not in the trash. */
  livePages(): TreeNode[];
  isLive(id: NodeId): boolean;
  touch(id: NodeId): void;
}

export interface TreeHooks {
  /** A node was created or permanently deleted. */
  created(node: TreeNode): void;
  deleted(id: NodeId): void;
  changed(ids: NodeId[]): void;
}

const ROOT = "";

/** Tree with soft delete, mirroring the core's observable behaviour. */
export function createTreeStore(hooks: TreeHooks): TreeStore {
  const nodes = new Map<NodeId, TreeNode>();
  const order = new Map<string, NodeId[]>();
  /** Explicitly trashed nodes; their descendants are hidden with them. */
  const trashed = new Map<NodeId, number>();

  const key = (parent: NodeId | null) => parent ?? ROOT;
  const siblings = (parent: NodeId | null): NodeId[] => {
    const list = order.get(key(parent)) ?? [];
    order.set(key(parent), list);
    return list;
  };

  const need = (id: NodeId): TreeNode => {
    const node = nodes.get(id);
    if (!node) throw apiError("not_found", "That page or folder does not exist.");
    return node;
  };

  const isLive = (id: NodeId): boolean => {
    for (let cursor: NodeId | null = id; cursor !== null; cursor = nodes.get(cursor)?.parentId ?? null) {
      if (trashed.has(cursor) || !nodes.has(cursor)) return false;
    }
    return true;
  };

  const descendants = (id: NodeId): NodeId[] =>
    (order.get(id) ?? []).flatMap((child) => [child, ...descendants(child)]);

  const isDescendantOf = (id: NodeId, ancestor: NodeId): boolean => {
    for (let cursor = nodes.get(id)?.parentId ?? null; cursor !== null; cursor = nodes.get(cursor)?.parentId ?? null) {
      if (cursor === ancestor) return true;
    }
    return false;
  };

  function insertAt(parent: NodeId | null, id: NodeId, index: number | undefined): void {
    const list = siblings(parent);
    const live = list.filter(isLive);
    const anchor = index === undefined ? undefined : live[index];
    const at = anchor === undefined ? list.length : list.indexOf(anchor);
    list.splice(at, 0, id);
  }

  const detach = (id: NodeId) => {
    const list = siblings(nodes.get(id)?.parentId ?? null);
    const at = list.indexOf(id);
    if (at >= 0) list.splice(at, 1);
  };

  const touch = (id: NodeId) => {
    const node = nodes.get(id);
    if (node) node.updatedAt = Date.now();
  };

  const requireParent = (parentId: NodeId | null) => {
    if (parentId !== null && !isLive(parentId)) throw apiError("not_found", "The destination no longer exists.");
  };

  const flatten = (parent: NodeId | null, out: TreeNode[]): TreeNode[] => {
    for (const id of siblings(parent)) {
      const node = nodes.get(id);
      if (node && isLive(id)) {
        out.push({ ...node });
        flatten(id, out);
      }
    }
    return out;
  };

  function deletePermanently(id: NodeId): void {
    need(id);
    const gone = [id, ...descendants(id)];
    detach(id);
    for (const goneId of gone) {
      nodes.delete(goneId);
      order.delete(goneId);
      trashed.delete(goneId);
      hooks.deleted(goneId);
    }
    hooks.changed(gone);
  }

  function listTrash(): TrashEntry[] {
    const entries: TrashEntry[] = [];
    for (const [id, trashedAt] of trashed) {
      const node = nodes.get(id);
      const parent = node?.parentId ?? null;
      if (node && (parent === null || isLive(parent))) entries.push({ ...node, trashedAt });
    }
    return entries.sort((a, b) => b.trashedAt - a.trashedAt);
  }

  return {
    get: (id) => nodes.get(id),
    isLive,
    touch,
    livePages: () => [...nodes.values()].filter((node) => node.kind === "page" && isLive(node.id)),
    breadcrumb(id) {
      const titles: string[] = [];
      for (let cursor = nodes.get(id)?.parentId ?? null; cursor !== null; cursor = nodes.get(cursor)?.parentId ?? null) {
        titles.unshift(nodes.get(cursor)?.title ?? "");
      }
      return titles;
    },

    treeSnapshot: async () => flatten(null, []),

    async treeCreate({ kind, parentId, index, title }) {
      requireParent(parentId);
      const now = Date.now();
      const node: TreeNode = {
        id: randomId(kind),
        parentId,
        kind,
        title: title ?? "",
        icon: null,
        favorite: false,
        createdAt: now,
        updatedAt: now,
      };
      nodes.set(node.id, node);
      insertAt(parentId, node.id, index);
      hooks.created(node);
      hooks.changed([node.id]);
      return node.id;
    },

    async treeRename({ id, title }) {
      need(id).title = title;
      touch(id);
      hooks.changed([id]);
    },
    async treeSetIcon({ id, icon }) {
      need(id).icon = icon;
      hooks.changed([id]);
    },
    async treeSetFavorite({ id, favorite }) {
      need(id).favorite = favorite;
      hooks.changed([id]);
    },

    async treeMove({ id, parentId, index }) {
      const node = need(id);
      requireParent(parentId);
      if (parentId === id || (parentId !== null && isDescendantOf(parentId, id))) {
        throw apiError("cycle", "A page or folder cannot be moved into itself.");
      }
      detach(id);
      node.parentId = parentId;
      insertAt(parentId, id, index);
      touch(id);
      hooks.changed([id]);
    },

    async treeTrash({ id }) {
      need(id);
      trashed.set(id, Date.now());
      hooks.changed([id, ...descendants(id)]);
    },
    async treeRestore({ id }) {
      need(id);
      if (!trashed.delete(id)) return;
      const parent = nodes.get(id)?.parentId ?? null;
      if (parent !== null && !isLive(parent)) {
        detach(id);
        need(id).parentId = null;
        siblings(null).push(id);
      }
      hooks.changed([id, ...descendants(id)]);
    },
    treeDeletePermanently: async ({ id }) => deletePermanently(id),
    treeListTrash: async () => listTrash(),
    treeEmptyTrash: async () => {
      for (const entry of listTrash()) deletePermanently(entry.id);
    },
  };
}
