import { createSignal } from "solid-js";
import type { NodeId } from "../api/contract.ts";
import { isStringArray, readJson, writeJson } from "../lib/storage.ts";

const KEY = "ne.recent-pages";
const LIMIT = 8;

export interface Recents {
  ids: () => NodeId[];
  touch(id: NodeId): void;
}

/** Most recently opened pages, newest first. */
export function createRecents(): Recents {
  const [ids, setIds] = createSignal<NodeId[]>(readJson(KEY, [], isStringArray));
  return {
    ids,
    touch(id) {
      const next = [id, ...ids().filter((existing) => existing !== id)].slice(0, LIMIT);
      setIds(next);
      writeJson(KEY, next);
    },
  };
}
