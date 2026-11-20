import type { LoroDoc } from "loro-crdt";
import type { CoreApi, NodeId, PageHistory, PageVersion } from "./contract.ts";
import { apiError } from "./errors.ts";
import { copyChildren, ensureNotEmpty, rootChildren, toPreview } from "./memory-doc-ops.ts";

type HistoryMethods = Pick<CoreApi, "pageHistory" | "pageVersionPreview" | "pageVersionRestore">;

export interface History extends HistoryMethods {
  /** Pretend the page arrived as a trimmed copy: versions are listed but cannot be previewed or restored. */
  setTrimmed(pageId: NodeId, trimmed: boolean): void;
}

interface Hooks {
  doc(id: NodeId): LoroDoc | undefined;
  publish(pageId: NodeId, update: Uint8Array): void;
  isLive(id: NodeId): boolean;
}

/** Consecutive changes by one author with gaps under five minutes form one version. */
const VERSION_GAP_SECONDS = 5 * 60;

interface Group {
  peer: string;
  firstCounter: number;
  lastCounter: number;
  first: number;
  last: number;
  changes: number;
  operations: number;
  lamport: number;
}

function groupChanges(doc: LoroDoc): Group[] {
  const flat = [...doc.getAllChanges()].flatMap(([peer, changes]) => changes.map((change) => ({ peer: String(peer), change })));
  flat.sort((a, b) => a.change.lamport - b.change.lamport || a.change.timestamp - b.change.timestamp);
  const groups: Group[] = [];
  for (const { peer, change } of flat) {
    const open = groups.at(-1);
    const stamp = change.timestamp;
    const joins =
      open !== undefined &&
      open.peer === peer &&
      (stamp === 0 ? open.last === 0 : open.last !== 0 && stamp - open.last < VERSION_GAP_SECONDS);
    if (open && joins) {
      open.lastCounter = change.counter + change.length - 1;
      open.last = stamp;
      open.changes += 1;
      open.operations += change.length;
      continue;
    }
    groups.push({
      peer,
      firstCounter: change.counter,
      lastCounter: change.counter + change.length - 1,
      first: stamp,
      last: stamp,
      changes: 1,
      operations: change.length,
      lamport: change.lamport,
    });
  }
  return groups;
}

const toVersion = (group: Group): PageVersion => ({
  id: `${group.peer}:${group.lastCounter}`,
  author: group.peer,
  startedAt: group.first === 0 ? null : group.first * 1000,
  endedAt: group.last === 0 ? null : group.last * 1000,
  changes: group.changes,
  operations: group.operations,
});

export function createHistory(hooks: Hooks): History {
  const trimmed = new Set<NodeId>();

  const need = (id: NodeId): LoroDoc => {
    const doc = hooks.doc(id);
    if (!doc || !hooks.isLive(id)) throw apiError("not_found", "That page is not available.");
    return doc;
  };

  const fork = (id: NodeId, versionId: string): LoroDoc => {
    if (trimmed.has(id)) throw apiError("history_unavailable", "Older history is not available for this page.");
    const [peer, counter] = versionId.split(":");
    const doc = need(id);
    if (!peer || counter === undefined || Number.isNaN(Number(counter))) throw apiError("not_found", "That version does not exist.");
    try {
      return doc.forkAt([{ peer: peer as `${number}`, counter: Number(counter) }]);
    } catch {
      throw apiError("not_found", "That version does not exist.");
    }
  };

  return {
    setTrimmed: (pageId, value) => void (value ? trimmed.add(pageId) : trimmed.delete(pageId)),

    async pageHistory({ id }): Promise<PageHistory> {
      const versions = groupChanges(need(id)).map(toVersion).reverse();
      const oldest = versions.map((version) => version.startedAt).filter((at): at is number => at !== null);
      return {
        versions,
        trimmed: trimmed.has(id),
        previewable: !trimmed.has(id),
        oldestAt: oldest.length > 0 ? Math.min(...oldest) : null,
      };
    },

    async pageVersionPreview({ id, versionId }) {
      return toPreview(fork(id, versionId));
    },

    async pageVersionRestore({ id, versionId }) {
      const past = fork(id, versionId);
      const doc = need(id);
      const before = doc.version();
      const list = rootChildren(doc);
      list.delete(0, list.length);
      copyChildren(rootChildren(past), list);
      ensureNotEmpty(doc);
      doc.commit();
      hooks.publish(id, doc.export({ mode: "update", from: before }));
    },
  };
}
