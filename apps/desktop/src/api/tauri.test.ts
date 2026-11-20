import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";

const invoke = vi.fn();
const listen = vi.fn();
vi.mock("@tauri-apps/api/core", () => ({ invoke: (...args: unknown[]) => invoke(...args) }));
vi.mock("@tauri-apps/api/event", () => ({ listen: (...args: unknown[]) => listen(...args) }));

const { tauriApi, tauriEvents } = await import("./tauri.ts");
const { isTauri, loadCore } = await import("./index.ts");

beforeEach(() => {
  invoke.mockReset();
  listen.mockReset();
});

afterEach(() => {
  delete (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__;
  window.history.replaceState(null, "", "/");
});

describe("Tauri bridge", () => {
  it("calls snake_case commands with camelCase arguments", async () => {
    invoke.mockResolvedValue(undefined);
    await tauriApi.treeMove({ id: "n1", parentId: null, index: 2 });
    expect(invoke).toHaveBeenCalledWith("tree_move", { id: "n1", parentId: null, index: 2 });
    await tauriApi.vaultGeneratePassword({ mode: "characters", length: 20 });
    expect(invoke).toHaveBeenLastCalledWith("vault_generate_password", { mode: "characters", length: 20 });
    await tauriApi.treeSnapshot();
    expect(invoke).toHaveBeenLastCalledWith("tree_snapshot", undefined);
  });

  it("sends binary as plain byte arrays", async () => {
    invoke.mockResolvedValue(undefined);
    await tauriApi.pageApplyUpdate({ id: "p", update: new Uint8Array([1, 2, 3]) });
    expect(invoke).toHaveBeenCalledWith("page_apply_update", { id: "p", update: [1, 2, 3] });
    await tauriApi.attachmentAddBytes({ bytes: new Uint8Array([9]), mime: "image/png" });
    expect(invoke).toHaveBeenLastCalledWith("attachment_add_bytes", { bytes: [9], mime: "image/png" });
  });

  it("returns binary results as Uint8Array whatever shape the core uses", async () => {
    invoke.mockResolvedValueOnce({ snapshot: [4, 5, 6] });
    const page = await tauriApi.pageOpen({ id: "p" });
    expect(page.snapshot).toBeInstanceOf(Uint8Array);
    expect([...page.snapshot]).toEqual([4, 5, 6]);
    expect(invoke).toHaveBeenCalledWith("page_open", { id: "p" });
    invoke.mockResolvedValueOnce({ snapshot: new Uint8Array([7]).buffer });
    expect([...(await tauriApi.pageOpen({ id: "p" })).snapshot]).toEqual([7]);
  });

  it("turns every kind of rejection into an ApiError", async () => {
    invoke.mockRejectedValueOnce({ code: "wrong_password", message: "Wrong password." });
    await expect(tauriApi.vaultUnlock({ password: "x" })).rejects.toEqual({ code: "wrong_password", message: "Wrong password." });
    invoke.mockRejectedValueOnce("plain string failure");
    await expect(tauriApi.treeSnapshot()).rejects.toEqual({ code: "unknown", message: "plain string failure" });
    invoke.mockRejectedValueOnce(new Error("exploded"));
    await expect(tauriApi.treeSnapshot()).rejects.toEqual({ code: "unknown", message: "exploded" });
    invoke.mockRejectedValueOnce(undefined);
    const failure = await tauriApi.treeSnapshot().catch((error: unknown) => error);
    expect(failure).toMatchObject({ code: "unknown" });
  });

  it("subscribes to events and decodes remote page updates", async () => {
    const stop = vi.fn();
    let handler: ((message: { payload: unknown }) => void) | undefined;
    listen.mockImplementation((name: string, fn: typeof handler) => {
      expect(name).toBe("page:remote-update");
      handler = fn;
      return Promise.resolve(stop);
    });
    const seen = vi.fn();
    const unlisten = await tauriEvents.on("page:remote-update", seen);
    handler?.({ payload: { pageId: "p", update: [1, 2] } });
    const [{ update, pageId }] = seen.mock.calls[0] as [{ pageId: string; update: Uint8Array }];
    expect(pageId).toBe("p");
    expect(update).toBeInstanceOf(Uint8Array);
    unlisten();
    expect(stop).toHaveBeenCalled();
  });

  it("passes other event payloads through untouched", async () => {
    let handler: ((message: { payload: unknown }) => void) | undefined;
    listen.mockImplementation((_name: string, fn: typeof handler) => {
      handler = fn;
      return Promise.resolve(() => undefined);
    });
    const seen = vi.fn();
    await tauriEvents.on("vault:state", seen);
    handler?.({ payload: { state: "locked" } });
    expect(seen).toHaveBeenCalledWith({ state: "locked" });
  });
});

describe("choosing the implementation", () => {
  it("uses the real bridge inside Tauri", async () => {
    (window as unknown as Record<string, unknown>).__TAURI_INTERNALS__ = {};
    expect(isTauri()).toBe(true);
    const core = await loadCore();
    expect(core.preview).toBe(false);
    expect(core.api).toBe(tauriApi);
    expect(core.events).toBe(tauriEvents);
  });

  it("uses the in-memory core in a browser during development", async () => {
    expect(isTauri()).toBe(false);
    const core = await loadCore();
    expect(core.preview).toBe(true);
    expect(core.api).not.toBe(tauriApi);
    expect(await core.api.sessionStatus()).toEqual({ state: "empty" });
  });

  it("starts with sample pages only when asked for with ?demo", async () => {
    window.history.replaceState(null, "", "/?demo");
    const core = await loadCore();
    expect((await core.api.sessionStatus()).state).toBe("local");
    const titles = (await core.api.treeSnapshot()).map((node) => node.title);
    expect(titles).toEqual(expect.arrayContaining(["Welcome", "Projects", "Garden plan"]));
  });
});

describe("Tauri bridge: databases, templates, history, import, synced blocks, reminders, updates", () => {
  const commands: [keyof typeof tauriApi, string][] = [
    ["dbCreate", "db_create"],
    ["dbSchema", "db_schema"],
    ["dbQuery", "db_query"],
    ["dbPropertyAdd", "db_property_add"],
    ["dbPropertyRename", "db_property_rename"],
    ["dbPropertyDelete", "db_property_delete"],
    ["dbPropertyMove", "db_property_move"],
    ["dbPropertyChangeType", "db_property_change_type"],
    ["dbRelationAdd", "db_relation_add"],
    ["dbOptionAdd", "db_option_add"],
    ["dbOptionRename", "db_option_rename"],
    ["dbOptionColor", "db_option_color"],
    ["dbOptionDelete", "db_option_delete"],
    ["dbOptionMove", "db_option_move"],
    ["dbRowCreate", "db_row_create"],
    ["dbCellSet", "db_cell_set"],
    ["dbCardMove", "db_card_move"],
    ["dbViewAdd", "db_view_add"],
    ["dbViewSave", "db_view_save"],
    ["dbViewDelete", "db_view_delete"],
    ["dbViewMove", "db_view_move"],
    ["templateList", "template_list"],
    ["templateCreateFromPage", "template_create_from_page"],
    ["templateCreateForDatabase", "template_create_for_database"],
    ["templateRename", "template_rename"],
    ["templateSetDefaults", "template_set_defaults"],
    ["templateDelete", "template_delete"],
    ["templateInstantiatePage", "template_instantiate_page"],
    ["templateInstantiateRow", "template_instantiate_row"],
    ["pageHistory", "page_history"],
    ["pageVersionPreview", "page_version_preview"],
    ["pageVersionRestore", "page_version_restore"],
    ["importStructured", "import_structured"],
    ["syncedCreate", "synced_create"],
    ["syncedInsert", "synced_insert"],
    ["syncedApplyUpdate", "synced_apply_update"],
    ["syncedClose", "synced_close"],
    ["syncedUnsync", "synced_unsync"],
    ["syncedOccurrences", "synced_occurrences"],
    ["syncedList", "synced_list"],
    ["reminderAdd", "reminder_add"],
    ["reminderList", "reminder_list"],
    ["reminderSetDone", "reminder_set_done"],
    ["reminderSnooze", "reminder_snooze"],
    ["reminderReschedule", "reminder_reschedule"],
    ["reminderSetNote", "reminder_set_note"],
    ["reminderDelete", "reminder_delete"],
    ["updateStatus", "update_status"],
    ["updateCheck", "update_check"],
    ["updateDownload", "update_download"],
    ["updateInstall", "update_install"],
  ];

  it.each(commands)("%s calls %s with its arguments as they are", async (method, command) => {
    invoke.mockResolvedValue(undefined);
    const args = { id: "n1", extra: [1, 2] };
    await (tauriApi[method] as (args: unknown) => Promise<unknown>)(args);
    expect(invoke).toHaveBeenCalledWith(command, args);
  });

  it("returns the snapshot of a shared document as bytes", async () => {
    invoke.mockResolvedValueOnce({ snapshot: [1, 2, 3] });
    const opened = await tauriApi.syncedOpen({ syncId: "0@abc" });
    expect(opened.snapshot).toBeInstanceOf(Uint8Array);
    expect(invoke).toHaveBeenCalledWith("synced_open", { syncId: "0@abc" });
  });

  it("passes the new events through, decoding the bytes of a shared document's update", async () => {
    let deliver: (message: { payload: unknown }) => void = () => {};
    listen.mockImplementation((_name: string, handler: typeof deliver) => {
      deliver = handler;
      return Promise.resolve(() => {});
    });
    const seen: unknown[] = [];
    await tauriEvents.on("reminder:due", (payload) => seen.push(payload));
    deliver({ payload: { reminders: [], nativeShown: true } });
    expect(seen).toEqual([{ reminders: [], nativeShown: true }]);
    const updates: Uint8Array[] = [];
    await tauriEvents.on("page:remote-update", (payload) => updates.push(payload.update));
    deliver({ payload: { pageId: "0@abc", update: [9, 8] } });
    expect([...(updates[0] ?? [])]).toEqual([9, 8]);
    expect(listen).toHaveBeenCalledWith("reminder:due", expect.any(Function));
  });
});

