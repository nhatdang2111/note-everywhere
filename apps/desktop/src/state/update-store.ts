import { createSignal } from "solid-js";
import type { Core } from "../api/index.ts";
import type { UpdateStatus } from "../api/contract.ts";

export interface UpdateStore {
  status: () => UpdateStatus | null;
  /** Progress of a running download, or null. */
  progress: () => { receivedBytes: number; totalBytes: number | null } | null;
  refresh(): Promise<void>;
  check(): Promise<void>;
  download(): Promise<void>;
  /** Applies the downloaded version and restarts. Only call after the person confirmed. */
  install(): Promise<void>;
}

/** The self-update state of the app. Nothing here ever downloads or installs on its own. */
export function createUpdateStore(core: Core, onError: (error: unknown) => void): UpdateStore {
  const [status, setStatus] = createSignal<UpdateStatus | null>(null);
  const [progress, setProgress] = createSignal<{ receivedBytes: number; totalBytes: number | null } | null>(null);

  void core.events.on("update:status", (next) => {
    setStatus(next);
    if (next.state !== "downloading") setProgress(null);
  });
  void core.events.on("update:progress", setProgress);

  const attempt = async (run: () => Promise<UpdateStatus | void>) => {
    try {
      const next = await run();
      if (next) setStatus(next);
    } catch (error) {
      onError(error);
    }
  };

  return {
    status,
    progress,
    refresh: () => attempt(() => core.api.updateStatus()),
    check: () => attempt(() => core.api.updateCheck()),
    download: () => attempt(() => core.api.updateDownload()),
    install: () => attempt(() => core.api.updateInstall()),
  };
}
