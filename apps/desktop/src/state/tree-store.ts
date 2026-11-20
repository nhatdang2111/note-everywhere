import { createMemo, createSignal } from "solid-js";
import type { Core } from "../api/index.ts";
import type { ApiError, CreateNodeArgs, MoveNodeArgs, NodeId, TrashEntry, TreeNode } from "../api/contract.ts";
import { toApiError } from "../api/errors.ts";
import { isStringArray, readJson, writeJson } from "../lib/storage.ts";
import { buildIndex, visibleRows, type TreeIndex, type TreeRow } from "./tree-model.ts";

const EXPANDED_KEY = "ne.tree.expanded";

export interface TreeStore {
  nodes: () => TreeNode[];
  index: () => TreeIndex;
  rows: () => TreeRow[];
  favorites: () => TreeNode[];
  trash: () => TrashEntry[];
  loaded: () => boolean;
  error: () => ApiError | null;
  expanded: () => ReadonlySet<NodeId>;
  setExpanded(id: NodeId, open?: boolean): void;
  refresh(): Promise<void>;
  refreshTrash(): Promise<void>;
  create(args: CreateNodeArgs): Promise<NodeId>;
  rename(id: NodeId, title: string): Promise<void>;
  setFavorite(id: NodeId, favorite: boolean): Promise<void>;
  move(args: MoveNodeArgs): Promise<void>;
  moveToTrash(id: NodeId): Promise<void>;
  restore(id: NodeId): Promise<void>;
  deletePermanently(id: NodeId): Promise<void>;
  emptyTrash(): Promise<void>;
  /** "ok" for pages in the tree, "trashed" for pages in the trash, otherwise "missing". */
  pageState(id: NodeId): "ok" | "trashed" | "missing";
  title(id: NodeId): string;
}

export function createTreeStore(core: Core): TreeStore {
  const [nodes, setNodes] = createSignal<TreeNode[]>([]);
  const [trash, setTrash] = createSignal<TrashEntry[]>([]);
  const [loaded, setLoaded] = createSignal(false);
  const [error, setError] = createSignal<ApiError | null>(null);
  const [expanded, setExpandedSet] = createSignal<ReadonlySet<NodeId>>(new Set(readJson(EXPANDED_KEY, [], isStringArray)));

  const index = createMemo(() => buildIndex(nodes()));
  const rows = createMemo(() => visibleRows(index(), expanded()));
  const favorites = createMemo(() => nodes().filter((node) => node.favorite));
  const trashedIds = createMemo(() => new Set(trash().map((entry) => entry.id)));

  async function refresh(): Promise<void> {
    try {
      setNodes(await core.api.treeSnapshot());
      setError(null);
    } catch (failure) {
      setError(toApiError(failure));
    } finally {
      setLoaded(true);
    }
  }

  async function refreshTrash(): Promise<void> {
    try {
      setTrash(await core.api.treeListTrash());
    } catch (failure) {
      setError(toApiError(failure));
    }
  }

  const setExpanded = (id: NodeId, open?: boolean) => {
    const next = new Set(expanded());
    const want = open ?? !next.has(id);
    if (want) next.add(id);
    else next.delete(id);
    setExpandedSet(next);
    writeJson(EXPANDED_KEY, [...next]);
  };

  // Changes from sync or other windows arrive here; our own commands also echo
  // through it, so bursts are folded into one refresh.
  let scheduled = false;
  void core.events.on("tree:changed", () => {
    if (scheduled) return;
    scheduled = true;
    queueMicrotask(() => {
      scheduled = false;
      void refresh();
      if (trash().length > 0) void refreshTrash();
    });
  });

  /** Run a command, then re-read what it may have changed. */
  async function mutate<T>(run: () => Promise<T>, withTrash = false): Promise<T> {
    const result = await run();
    await Promise.all([refresh(), withTrash ? refreshTrash() : Promise.resolve()]);
    return result;
  }

  return {
    nodes,
    index,
    rows,
    favorites,
    trash,
    loaded,
    error,
    expanded,
    setExpanded,
    refresh,
    refreshTrash,
    async create(args) {
      const id = await mutate(() => core.api.treeCreate(args));
      if (args.parentId) setExpanded(args.parentId, true);
      return id;
    },
    rename: (id, title) => mutate(() => core.api.treeRename({ id, title })),
    setFavorite: (id, favorite) => mutate(() => core.api.treeSetFavorite({ id, favorite })),
    move: async (args) => {
      await mutate(() => core.api.treeMove(args));
      if (args.parentId) setExpanded(args.parentId, true);
    },
    moveToTrash: (id) => mutate(() => core.api.treeTrash({ id }), true),
    restore: (id) => mutate(() => core.api.treeRestore({ id }), true),
    deletePermanently: (id) => mutate(() => core.api.treeDeletePermanently({ id }), true),
    emptyTrash: () => mutate(() => core.api.treeEmptyTrash(), true),
    pageState(id) {
      if (index().byId.has(id)) return "ok";
      return trashedIds().has(id) ? "trashed" : "missing";
    },
    title: (id) => index().byId.get(id)?.title ?? "",
  };
}
