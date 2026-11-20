import type { NodeId } from "../../api/contract.ts";
import { fold, matchesQuery } from "../../lib/text.ts";
import { ancestors, type TreeIndex } from "../../state/tree-model.ts";

export interface PageCandidate {
  id: NodeId;
  title: string;
  /** Ancestor titles, for telling apart pages with the same name. */
  path: string;
}

interface Options {
  exclude?: NodeId;
  limit: number;
  /** Most recently opened pages, newest first; they lead when nothing is typed. */
  recent?: readonly NodeId[];
  untitled: string;
}

/**
 * Pages to offer in a link picker. With no query, recent pages come first;
 * with a query, titles that start with it come before titles that merely
 * contain it. Folders are never offered: only pages can be linked.
 */
export function pageCandidates(index: TreeIndex, query: string, options: Options): PageCandidate[] {
  const pages = [...index.byId.values()].filter((node) => node.kind === "page" && node.id !== options.exclude);
  const needle = fold(query.trim());
  const recentRank = (id: NodeId) => {
    const at = options.recent?.indexOf(id) ?? -1;
    return at < 0 ? Number.MAX_SAFE_INTEGER : at;
  };
  const scored = pages
    .filter((node) => !needle || matchesQuery(node.title || options.untitled, needle))
    .map((node) => {
      const title = node.title || options.untitled;
      const startsWith = needle !== "" && fold(title).startsWith(needle);
      return { node, title, rank: needle ? (startsWith ? 0 : 1) : 0, recent: recentRank(node.id) };
    })
    .sort((a, b) => a.rank - b.rank || a.recent - b.recent || a.title.localeCompare(b.title));
  return scored.slice(0, options.limit).map(({ node, title }) => ({
    id: node.id,
    title,
    path: ancestors(index, node.id)
      .map((ancestor) => ancestor.title || options.untitled)
      .join(" / "),
  }));
}
