import type { CoreEventMap, CoreEvents } from "./contract.ts";

type Handler = (payload: never) => void;

export interface Bus {
  emit<K extends keyof CoreEventMap>(event: K, payload: CoreEventMap[K]): void;
  events: CoreEvents;
}

/** Minimal event hub implementing `CoreEvents` for the in-memory core. */
export function createBus(): Bus {
  const handlers = new Map<string, Set<Handler>>();
  return {
    emit(event, payload) {
      for (const handler of [...(handlers.get(event) ?? [])]) {
        (handler as (value: typeof payload) => void)(payload);
      }
    },
    events: {
      on(event, handler) {
        const set = handlers.get(event) ?? new Set<Handler>();
        handlers.set(event, set);
        set.add(handler as Handler);
        return Promise.resolve(() => {
          set.delete(handler as Handler);
        });
      },
    },
  };
}

/** Hooks the pieces of the in-memory core use to reach each other. */
export interface MemoryOptions {
  /** Milliseconds before queued updates count as synced. Default 400. */
  syncDelayMs?: number;
  /** Re-authentication grace period for vault secrets. Default 60 000. */
  reauthGraceMs?: number;
  /** Seconds the clipboard keeps a copied secret. Default 30. */
  clipboardSeconds?: number;
  /** Whether this build offers transcription. Default true. */
  voiceAvailable?: boolean;
  /** Native file picker stand-in. The default opens a browser file input. */
  pickFiles?: (accept: string, multiple: boolean, directory?: boolean) => Promise<File[]>;
  /** Native save stand-in. The default downloads a file. */
  saveFile?: (name: string, content: string) => void;
  /** Clock stand-in for reminders; defaults to `Date.now`. */
  now?: () => number;
  /** Whether the core "shows a native notification" for due reminders (reported as `nativeShown`). Default false. */
  nativeReminders?: boolean;
  /** Report the updater as unavailable with this reason (a build without an updater key). */
  updatesUnavailable?: string;
}

export const MEMORY_SETUP_CODE = "preview-setup-code";
export const MIN_PASSWORD_LENGTH = 10;

export function randomId(prefix: string): string {
  const bytes = new Uint8Array(8);
  crypto.getRandomValues(bytes);
  return `${prefix}-${Array.from(bytes, (b) => b.toString(16).padStart(2, "0")).join("")}`;
}

const ALPHABET = "ABCDEFGHJKLMNPQRSTUVWXYZ23456789";

/** A recovery key in the shape `XXXX-XXXX-...` (six groups). Synthetic, preview only. */
export function randomRecoveryKey(): string {
  const bytes = new Uint8Array(24);
  crypto.getRandomValues(bytes);
  const chars = Array.from(bytes, (b) => ALPHABET[b % ALPHABET.length]).join("");
  return chars.match(/.{4}/g)?.join("-") ?? chars;
}
