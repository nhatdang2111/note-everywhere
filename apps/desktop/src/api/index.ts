import type { CoreApi, CoreEvents } from "./contract.ts";

export interface Core {
  api: CoreApi;
  events: CoreEvents;
  /** True when running without the Rust core (browser preview with the in-memory API). */
  preview: boolean;
}

export const isTauri = (): boolean => typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;

/**
 * Pick the implementation by detecting the Tauri runtime. The in-memory core is
 * only reachable in development: `import.meta.env.DEV` is replaced by `false`
 * in a production bundle, which removes the branch and the dynamic imports.
 */
export async function loadCore(): Promise<Core> {
  if (isTauri()) {
    const { tauriApi, tauriEvents } = await import("./tauri.ts");
    return { api: tauriApi, events: tauriEvents, preview: false };
  }
  if (import.meta.env.DEV) {
    const [{ createMemoryCore }, { seedPreview }] = await Promise.all([import("./memory.ts"), import("./preview-seed.ts")]);
    const memory = createMemoryCore();
    await seedPreview(memory.api, new URLSearchParams(window.location.search));
    return { api: memory.api, events: memory.events, preview: true };
  }
  throw new Error("Note Everywhere needs its desktop shell. Open the installed app instead of this page.");
}
