import { createSignal } from "solid-js";
import type { Core } from "../api/index.ts";
import type { SyncStatus } from "../api/contract.ts";

const IDLE: SyncStatus = { state: "disabled", pendingUpdates: 0, lastSyncedAt: null, lastError: null };

export interface SyncStore {
  status: () => SyncStatus;
  refresh(): Promise<void>;
  syncNow(): Promise<void>;
}

export function createSyncStore(core: Core, onError: (error: unknown) => void): SyncStore {
  const [status, setStatus] = createSignal<SyncStatus>(IDLE);
  void core.events.on("sync:status", setStatus);
  return {
    status,
    async refresh() {
      try {
        setStatus(await core.api.syncStatus());
      } catch (error) {
        onError(error);
      }
    },
    async syncNow() {
      try {
        await core.api.syncNow();
      } catch (error) {
        onError(error);
      }
    },
  };
}
