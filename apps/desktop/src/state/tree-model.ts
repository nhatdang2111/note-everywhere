import type { MoveNodeArgs, NodeId, TreeNode } from "../api/contract.ts";

/** Pure tree logic shared by the sidebar, search and page pickers. */

export interface TreeIndex {
  byId: Map<NodeId, TreeNode>;
  /** Children in display order, keyed by parent id (`null` for the top level). */
  children: Map<NodeId | null, NodeId[]>;
}

export interface TreeRow {
  node: TreeNode;
  depth: number;
  hasChildren: boolean;
  expanded: boolean;
}

export function buildIndex(snapshot: readonly TreeNode[]): TreeIndex {
  const byId = new Map<NodeId, TreeNode>();
  const children = new Map<NodeId | null, NodeId[]>();
  for (const node of snapshot) {
    byId.set(node.id, node);
    const siblings = children.get(node.parentId) ?? [];
    siblings.push(node.id);
    children.set(node.parentId, siblings);
  }
  return { byId, children };
}

/** Rows to render: depth-first, descending only into expanded nodes. */
export function visibleRows(index: TreeIndex, expanded: ReadonlySet<NodeId>): TreeRow[] {
  const rows: TreeRow[] = [];
  const visit = (parent: NodeId | null, depth: number) => {
    for (const id of index.children.get(parent) ?? []) {
      const node = index.byId.get(id);
      if (!node) continue;
      const hasChildren = (index.children.get(id)?.length ?? 0) > 0;
      const open = hasChildren && expanded.has(id);
      rows.push({ node, depth, hasChildren, expanded: open });
      if (open) visit(id, depth + 1);
    }
  };
  visit(null, 0);
  return rows;
}

export function isDescendant(index: TreeIndex, id: NodeId, ancestorId: NodeId): boolean {
  for (let cursor = index.byId.get(id)?.parentId ?? null; cursor !== null; cursor = index.byId.get(cursor)?.parentId ?? null) {
    if (cursor === ancestorId) return true;
  }
  return false;
}

/** Ancestors of `id`, outermost first. */
export function ancestors(index: TreeIndex, id: NodeId): TreeNode[] {
  const chain: TreeNode[] = [];
  for (let cursor = index.byId.get(id)?.parentId ?? null; cursor !== null; cursor = index.byId.get(cursor)?.parentId ?? null) {
    const node = index.byId.get(cursor);
    if (!node) break;
    chain.unshift(node);
  }
  return chain;
}

export type DropZone = "before" | "inside" | "after";

/**
 * Turn a drop of `dragId` on `targetId` into a move, or null when the drop is
 * not allowed (onto itself or one of its own descendants) or changes nothing.
 *
 * `index` is the position among the destination's children after the dragged
 * node has been taken out of its old place.
 */
export function planDrop(index: TreeIndex, dragId: NodeId, targetId: NodeId, zone: DropZone): MoveNodeArgs | null {
  const dragged = index.byId.get(dragId);
  const target = index.byId.get(targetId);
  if (!dragged || !target) return null;
  if (dragId === targetId || isDescendant(index, targetId, dragId)) return null;

  if (zone === "inside") {
    if (dragged.parentId === targetId && lastOf(index, targetId) === dragId) return null;
    return { id: dragId, parentId: targetId };
  }

  const parentId = target.parentId;
  const siblings = (index.children.get(parentId) ?? []).filter((id) => id !== dragId);
  const at = siblings.indexOf(targetId) + (zone === "after" ? 1 : 0);
  const current = (index.children.get(parentId) ?? []).indexOf(dragId);
  if (dragged.parentId === parentId && current === at) return null;
  return { id: dragId, parentId, index: at };
}

const lastOf = (index: TreeIndex, parent: NodeId): NodeId | undefined => index.children.get(parent)?.at(-1);

/** Keyboard alternatives to dragging: one step up or down among siblings, or out to the grandparent. */
export function planStep(index: TreeIndex, id: NodeId, step: "up" | "down" | "out"): MoveNodeArgs | null {
  const node = index.byId.get(id);
  if (!node) return null;
  const siblings = index.children.get(node.parentId) ?? [];
  const at = siblings.indexOf(id);
  if (step === "up") return at > 0 ? { id, parentId: node.parentId, index: at - 1 } : null;
  if (step === "down") return at >= 0 && at < siblings.length - 1 ? { id, parentId: node.parentId, index: at + 1 } : null;
  if (node.parentId === null) return null;
  const parent = index.byId.get(node.parentId);
  const grand = parent?.parentId ?? null;
  const parentAt = (index.children.get(grand) ?? []).indexOf(node.parentId);
  return { id, parentId: grand, index: parentAt + 1 };
}

/** Which third of a row the pointer is in decides before, inside or after. */
export function zoneFor(offsetY: number, height: number): DropZone {
  const ratio = height > 0 ? offsetY / height : 0.5;
  if (ratio < 0.28) return "before";
  if (ratio > 0.72) return "after";
  return "inside";
}
