import { invoke } from "@tauri-apps/api/core";
import { listen } from "@tauri-apps/api/event";
import type { CoreApi, CoreEventMap, CoreEvents, OpenPage } from "./contract.ts";
import { toApiError } from "./errors.ts";

/** `treeMove` becomes `tree_move`. */
export function toSnakeCase(name: string): string {
  return name.replace(/[A-Z]/g, (letter) => `_${letter.toLowerCase()}`);
}

/**
 * Tauri serialises arguments with JSON, which turns a `Uint8Array` into an
 * object keyed by index. Convert binary values to plain number arrays, which
 * serde reads as `Vec<u8>`.
 */
export function encodeArgs(value: unknown): unknown {
  if (value instanceof Uint8Array) return Array.from(value);
  if (Array.isArray(value)) return value.map(encodeArgs);
  if (value !== null && typeof value === "object") {
    return Object.fromEntries(Object.entries(value).map(([key, inner]) => [key, encodeArgs(inner)]));
  }
  return value;
}

/** Accept the binary shapes Tauri can return: raw buffer, byte array, or already a Uint8Array. */
export function decodeBytes(value: unknown): Uint8Array {
  if (value instanceof Uint8Array) return value;
  if (value instanceof ArrayBuffer) return new Uint8Array(value);
  if (Array.isArray(value)) return Uint8Array.from(value as number[]);
  throw toApiError({ code: "bad_payload", message: "The core returned an unexpected binary payload." });
}

async function call(command: string, args?: unknown): Promise<unknown> {
  try {
    return await invoke(command, args === undefined ? undefined : (encodeArgs(args) as Record<string, unknown>));
  } catch (error) {
    throw toApiError(error);
  }
}

/**
 * One typed forwarder per contract method. The cast is the single place where
 * the generic call is narrowed to the method's signature; `tauriApi` below is
 * an object literal typed as `CoreApi`, so a missing method is a compile error.
 */
function command<K extends keyof CoreApi>(name: K): CoreApi[K] {
  const cmd = toSnakeCase(name);
  return ((args?: unknown) => call(cmd, args)) as CoreApi[K];
}

export const tauriApi: CoreApi = {
  appVersion: command("appVersion"),

  sessionStatus: command("sessionStatus"),
  workspaceCreateLocal: command("workspaceCreateLocal"),
  signUp: command("signUp"),
  signIn: command("signIn"),
  recoverAccount: command("recoverAccount"),
  signOut: command("signOut"),
  changePassword: command("changePassword"),
  sessionsList: command("sessionsList"),
  sessionsRevoke: command("sessionsRevoke"),

  treeSnapshot: command("treeSnapshot"),
  treeCreate: command("treeCreate"),
  treeRename: command("treeRename"),
  treeSetIcon: command("treeSetIcon"),
  treeSetFavorite: command("treeSetFavorite"),
  treeMove: command("treeMove"),
  treeTrash: command("treeTrash"),
  treeRestore: command("treeRestore"),
  treeDeletePermanently: command("treeDeletePermanently"),
  treeListTrash: command("treeListTrash"),
  treeEmptyTrash: command("treeEmptyTrash"),

  async pageOpen(args): Promise<OpenPage> {
    const raw = (await call("page_open", args)) as { snapshot: unknown };
    return { snapshot: decodeBytes(raw.snapshot) };
  },
  pageApplyUpdate: command("pageApplyUpdate"),
  pageClose: command("pageClose"),
  pageBacklinks: command("pageBacklinks"),

  searchQuery: command("searchQuery"),

  attachmentAddBytes: command("attachmentAddBytes"),
  attachmentPick: command("attachmentPick"),
  attachmentUrl: command("attachmentUrl"),

  exportMarkdown: command("exportMarkdown"),
  importMarkdown: command("importMarkdown"),

  syncStatus: command("syncStatus"),
  syncNow: command("syncNow"),

  vaultState: command("vaultState"),
  vaultCreate: command("vaultCreate"),
  vaultUnlock: command("vaultUnlock"),
  vaultRecover: command("vaultRecover"),
  vaultLock: command("vaultLock"),
  vaultChangePassword: command("vaultChangePassword"),
  vaultList: command("vaultList"),
  vaultSave: command("vaultSave"),
  vaultDelete: command("vaultDelete"),
  vaultReveal: command("vaultReveal"),
  vaultCopy: command("vaultCopy"),
  vaultGeneratePassword: command("vaultGeneratePassword"),
  vaultImportCsv: command("vaultImportCsv"),

  voiceStatus: command("voiceStatus"),
  voiceDownloadModel: command("voiceDownloadModel"),
  voiceDeleteModel: command("voiceDeleteModel"),
  voiceSelectModel: command("voiceSelectModel"),
  voiceStart: command("voiceStart"),
  voiceStop: command("voiceStop"),
  voiceCancel: command("voiceCancel"),

  settingsGet: command("settingsGet"),
  settingsSet: command("settingsSet"),

  // Databases
  dbCreate: command("dbCreate"),
  dbSchema: command("dbSchema"),
  dbQuery: command("dbQuery"),
  dbPropertyAdd: command("dbPropertyAdd"),
  dbPropertyRename: command("dbPropertyRename"),
  dbPropertyDelete: command("dbPropertyDelete"),
  dbPropertyMove: command("dbPropertyMove"),
  dbPropertyChangeType: command("dbPropertyChangeType"),
  dbRelationAdd: command("dbRelationAdd"),
  dbOptionAdd: command("dbOptionAdd"),
  dbOptionRename: command("dbOptionRename"),
  dbOptionColor: command("dbOptionColor"),
  dbOptionDelete: command("dbOptionDelete"),
  dbOptionMove: command("dbOptionMove"),
  dbRowCreate: command("dbRowCreate"),
  dbCellSet: command("dbCellSet"),
  dbCardMove: command("dbCardMove"),
  dbViewAdd: command("dbViewAdd"),
  dbViewSave: command("dbViewSave"),
  dbViewDelete: command("dbViewDelete"),
  dbViewMove: command("dbViewMove"),

  // Templates
  templateList: command("templateList"),
  templateCreateFromPage: command("templateCreateFromPage"),
  templateCreateForDatabase: command("templateCreateForDatabase"),
  templateRename: command("templateRename"),
  templateSetDefaults: command("templateSetDefaults"),
  templateDelete: command("templateDelete"),
  templateInstantiatePage: command("templateInstantiatePage"),
  templateInstantiateRow: command("templateInstantiateRow"),

  // Page history
  pageHistory: command("pageHistory"),
  pageVersionPreview: command("pageVersionPreview"),
  pageVersionRestore: command("pageVersionRestore"),

  // Structured import
  importStructured: command("importStructured"),

  // Synced blocks
  async syncedOpen(args): Promise<OpenPage> {
    const raw = (await call("synced_open", args)) as { snapshot: unknown };
    return { snapshot: decodeBytes(raw.snapshot) };
  },
  syncedCreate: command("syncedCreate"),
  syncedInsert: command("syncedInsert"),
  syncedApplyUpdate: command("syncedApplyUpdate"),
  syncedClose: command("syncedClose"),
  syncedUnsync: command("syncedUnsync"),
  syncedOccurrences: command("syncedOccurrences"),
  syncedList: command("syncedList"),

  // Reminders
  reminderAdd: command("reminderAdd"),
  reminderList: command("reminderList"),
  reminderSetDone: command("reminderSetDone"),
  reminderSnooze: command("reminderSnooze"),
  reminderReschedule: command("reminderReschedule"),
  reminderSetNote: command("reminderSetNote"),
  reminderDelete: command("reminderDelete"),

  // Updates
  updateStatus: command("updateStatus"),
  updateCheck: command("updateCheck"),
  updateDownload: command("updateDownload"),
  updateInstall: command("updateInstall"),
};

export const tauriEvents: CoreEvents = {
  on<K extends keyof CoreEventMap>(event: K, handler: (payload: CoreEventMap[K]) => void): Promise<() => void> {
    return listen<CoreEventMap[K]>(event, (message) => {
      const payload = message.payload;
      if (event === "page:remote-update") {
        const update = payload as CoreEventMap["page:remote-update"];
        handler({ ...update, update: decodeBytes(update.update) } as CoreEventMap[K]);
      } else {
        handler(payload);
      }
    });
  },
};
