import type { CoreApi, UpdateStatus } from "./contract.ts";
import { apiError } from "./errors.ts";
import type { Bus } from "./memory-bus.ts";

type UpdateMethods = Pick<CoreApi, "updateStatus" | "updateCheck" | "updateDownload" | "updateInstall">;

/** Levers for tests and the preview; the real core learns about releases from the update server. */
export interface UpdatesControl {
  /** Pretend the update server announces a newer version; the next check finds it. */
  announceUpdate(version: string, notes?: string): void;
}

export interface Updates extends UpdateMethods {
  control: UpdatesControl;
}

const CURRENT_VERSION = "0.0.0-preview";

/** The update flow of the contract over a made-up release feed. Nothing is downloaded or installed. */
export function createUpdates(bus: Bus, now: () => number, unavailable?: string): Updates {
  let status: UpdateStatus = unavailable
    ? { state: "unavailable", currentVersion: CURRENT_VERSION, lastCheckedAt: null, message: unavailable }
    : { state: "idle", currentVersion: CURRENT_VERSION, lastCheckedAt: null, message: null };
  let announced: { version: string; notes?: string } | null = null;

  const publish = (patch: Partial<UpdateStatus>): UpdateStatus => {
    status = { ...status, ...patch };
    bus.emit("update:status", { ...status });
    return { ...status };
  };

  return {
    updateStatus: async () => ({ ...status }),
    async updateCheck() {
      if (status.state === "unavailable" || status.state === "ready") return { ...status };
      if (status.state === "checking" || status.state === "downloading") throw apiError("update_busy", "An update check or download is already running.");
      publish({ state: "checking" });
      await Promise.resolve();
      if (!announced) return publish({ state: "up_to_date", lastCheckedAt: now(), message: null });
      return publish({ state: "available", version: announced.version, notes: announced.notes, lastCheckedAt: now(), message: null });
    },
    async updateDownload() {
      if (status.state !== "available") throw apiError("no_update", "There is no update to download.");
      publish({ state: "downloading" });
      bus.emit("update:progress", { receivedBytes: 0, totalBytes: 1 });
      await Promise.resolve();
      bus.emit("update:progress", { receivedBytes: 1, totalBytes: 1 });
      return publish({ state: "ready" });
    },
    async updateInstall() {
      if (status.state !== "ready") throw apiError("update_not_ready", "Download the update first.");
    },
    control: {
      announceUpdate(version, notes) {
        announced = { version, notes };
      },
    },
  };
}
