import type { Core } from "../api/index.ts";
import { configureDoc, createPageDoc, type PageDoc, type SyncedBlockHost, type SyncedSession } from "../editor/index.ts";

export interface SyncedHub extends SyncedBlockHost {
  dispose(): void;
}

interface View {
  doc: PageDoc;
  deliver: (update: Uint8Array) => void;
}

interface OpenDocument {
  views: Set<View>;
  /** Updates to the core go out one at a time, in order. */
  chain: Promise<void>;
}

/**
 * Connects the synced-block views of every open page to the core.
 *
 * Each view gets its own Loro document so it can be bound to its own editor;
 * the hub keeps them equal. A local edit goes to the core and to the other
 * views of the same shared document; an update from another device (the
 * `page:remote-update` event with the shared document's id) goes to all views.
 */
export function createSyncedHub(core: Core, onError: (error: unknown) => void): SyncedHub {
  const open = new Map<string, OpenDocument>();
  const waiting = new Map<string, Set<() => void>>();
  let stopListening: (() => void) | undefined;
  let disposed = false;

  void core.events
    .on("page:remote-update", ({ pageId, update }) => {
      for (const retry of [...(waiting.get(pageId) ?? [])]) retry();
      for (const view of open.get(pageId)?.views ?? []) view.deliver(update);
    })
    .then((stop) => (disposed ? stop() : (stopListening = stop)));

  return {
    async open(syncId): Promise<SyncedSession> {
      const { snapshot } = await core.api.syncedOpen({ syncId });
      const entry = open.get(syncId) ?? { views: new Set<View>(), chain: Promise.resolve() };
      open.set(syncId, entry);
      const doc = createPageDoc();
      if (snapshot.byteLength > 0) doc.import(snapshot);
      const view: View = { doc, deliver: (update) => void doc.import(update) };
      entry.views.add(view);

      const stopLocal = doc.subscribeLocalUpdates((update) => {
        entry.chain = entry.chain
          .then(() => core.api.syncedApplyUpdate({ syncId, update }))
          .catch(onError);
        for (const other of entry.views) if (other !== view) other.deliver(update);
      });

      return {
        doc: configureDoc(doc),
        setDeliver(handler) {
          view.deliver = handler;
        },
        close() {
          stopLocal();
          entry.views.delete(view);
          if (entry.views.size === 0) {
            open.delete(syncId);
            void entry.chain.then(() => core.api.syncedClose({ syncId })).catch(() => undefined);
          }
        },
      };
    },

    async occurrences(syncId) {
      try {
        return (await core.api.syncedOccurrences({ syncId })).length;
      } catch {
        return null;
      }
    },

    retryWhenAvailable(syncId, retry) {
      const set = waiting.get(syncId) ?? new Set<() => void>();
      waiting.set(syncId, set);
      set.add(retry);
      return () => void set.delete(retry);
    },

    dispose() {
      disposed = true;
      stopListening?.();
    },
  };
}
