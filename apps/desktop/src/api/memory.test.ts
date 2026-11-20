import { describe, expect, it, vi } from "vitest";
import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import type { CoreEventMap, TreeNode } from "./contract.ts";
import { createMemoryCore, MEMORY_SETUP_CODE } from "./memory.ts";
import { appendParagraph } from "./page-doc.ts";
import { parseCsv } from "./memory-vault.ts";

const GOOD_PASSWORD = "correct horse battery";

async function rejection(promise: Promise<unknown>): Promise<{ code: string; message: string }> {
  try {
    await promise;
  } catch (error) {
    return error as { code: string; message: string };
  }
  throw new Error("expected the call to reject");
}

async function localCore(options = {}) {
  const core = createMemoryCore({ syncDelayMs: 0, ...options });
  await core.api.workspaceCreateLocal();
  return core;
}

const titles = (nodes: TreeNode[]) => nodes.map((node) => node.title);

describe("in-memory core: tree", () => {
  it("lists nodes depth-first with siblings in display order", async () => {
    const { api } = await localCore();
    const a = await api.treeCreate({ kind: "folder", parentId: null, title: "A" });
    await api.treeCreate({ kind: "page", parentId: a, title: "A1" });
    const b = await api.treeCreate({ kind: "page", parentId: null, title: "B" });
    await api.treeCreate({ kind: "page", parentId: a, title: "A0", index: 0 });
    await api.treeCreate({ kind: "page", parentId: b, title: "B1" });
    expect(titles(await api.treeSnapshot())).toEqual(["A", "A0", "A1", "B", "B1"]);
  });

  it("moves with the index counted after the node is taken out", async () => {
    const { api } = await localCore();
    const ids = [];
    for (const title of ["a", "b", "c", "d"]) ids.push(await api.treeCreate({ kind: "page", parentId: null, title }));
    await api.treeMove({ id: ids[0] as string, parentId: null, index: 2 });
    expect(titles(await api.treeSnapshot())).toEqual(["b", "c", "a", "d"]);
    await api.treeMove({ id: ids[3] as string, parentId: ids[1] as string });
    expect(titles(await api.treeSnapshot())).toEqual(["b", "d", "c", "a"]);
  });

  it("refuses to move a node into its own descendant", async () => {
    const { api } = await localCore();
    const a = await api.treeCreate({ kind: "folder", parentId: null, title: "A" });
    const b = await api.treeCreate({ kind: "folder", parentId: a, title: "B" });
    expect((await rejection(api.treeMove({ id: a, parentId: b }))).code).toBe("cycle");
    expect((await rejection(api.treeMove({ id: a, parentId: a }))).code).toBe("cycle");
    expect(titles(await api.treeSnapshot())).toEqual(["A", "B"]);
  });

  it("soft-deletes a subtree, restores it in place, and deletes for good", async () => {
    const { api } = await localCore();
    const first = await api.treeCreate({ kind: "page", parentId: null, title: "first" });
    const folder = await api.treeCreate({ kind: "folder", parentId: null, title: "folder" });
    await api.treeCreate({ kind: "page", parentId: folder, title: "inner" });
    await api.treeCreate({ kind: "page", parentId: null, title: "last" });

    await api.treeTrash({ id: folder });
    expect(titles(await api.treeSnapshot())).toEqual(["first", "last"]);
    expect(titles(await api.treeListTrash())).toEqual(["folder"]);

    await api.treeRestore({ id: folder });
    expect(titles(await api.treeSnapshot())).toEqual(["first", "folder", "inner", "last"]);

    await api.treeTrash({ id: first });
    await api.treeDeletePermanently({ id: first });
    expect(await api.treeListTrash()).toEqual([]);
    expect((await rejection(api.pageOpen({ id: first }))).code).toBe("not_found");
  });

  it("restores to the top level when the old parent is still in the trash", async () => {
    const { api } = await localCore();
    const parent = await api.treeCreate({ kind: "folder", parentId: null, title: "parent" });
    const child = await api.treeCreate({ kind: "page", parentId: parent, title: "child" });
    await api.treeTrash({ id: child });
    await api.treeTrash({ id: parent });
    await api.treeRestore({ id: child });
    const snapshot = await api.treeSnapshot();
    expect(snapshot.find((node) => node.id === child)?.parentId).toBeNull();
  });

  it("empties the trash and announces changes", async () => {
    const { api, events } = await localCore();
    const changed: string[][] = [];
    await events.on("tree:changed", ({ nodeIds }) => changed.push(nodeIds));
    const id = await api.treeCreate({ kind: "page", parentId: null, title: "x" });
    await api.treeTrash({ id });
    await api.treeEmptyTrash();
    expect(await api.treeListTrash()).toEqual([]);
    expect(changed.length).toBeGreaterThanOrEqual(3);
  });

  it("renames and favourites", async () => {
    const { api } = await localCore();
    const id = await api.treeCreate({ kind: "page", parentId: null, title: "old" });
    await api.treeRename({ id, title: "new" });
    await api.treeSetFavorite({ id, favorite: true });
    const [node] = await api.treeSnapshot();
    expect(node).toMatchObject({ title: "new", favorite: true });
  });
});

describe("in-memory core: pages, search and links", () => {
  it("opens a snapshot with one empty paragraph and applies editor updates", async () => {
    const { api, control } = await localCore();
    const id = await api.treeCreate({ kind: "page", parentId: null, title: "p" });
    const { snapshot } = await api.pageOpen({ id });
    const doc = new LoroDoc();
    doc.import(snapshot);
    const before = doc.version();
    const children = doc.getMap("doc").get("children") as LoroList;
    expect(children.length).toBe(1);
    appendParagraph(doc, "ghi chú về vườn");
    await api.pageApplyUpdate({ id, update: doc.export({ mode: "update", from: before }) });
    expect(JSON.stringify(control.pageDoc(id)?.getMap("doc").toJSON())).toContain("ghi chú về vườn");
  });

  it("rejects a corrupt update", async () => {
    const { api } = await localCore();
    const id = await api.treeCreate({ kind: "page", parentId: null });
    expect((await rejection(api.pageApplyUpdate({ id, update: new Uint8Array([1, 2, 3]) }))).code).toBe("bad_update");
  });

  async function pageWith(api: Awaited<ReturnType<typeof localCore>>["api"], control: Awaited<ReturnType<typeof localCore>>["control"], title: string, text: string, parentId: string | null = null) {
    const id = await api.treeCreate({ kind: "page", parentId, title });
    const doc = control.pageDoc(id) as LoroDoc;
    appendParagraph(doc, text);
    return id;
  }

  it("searches titles and text without caring about accents or case", async () => {
    const { api, control } = await localCore();
    const folder = await api.treeCreate({ kind: "folder", parentId: null, title: "Công việc" });
    await pageWith(api, control, "Họp tuần", "Nội dung ghi chú quan trọng", folder);
    await pageWith(api, control, "Ghi chú nhanh", "không liên quan");
    await pageWith(api, control, "Other", "nothing here");
    const hits = await api.searchQuery({ text: "ghi chu" });
    expect(hits.map((hit) => hit.title)).toEqual(["Ghi chú nhanh", "Họp tuần"]);
    expect(hits[1]?.breadcrumb).toEqual(["Công việc"]);
    const body = hits[1];
    expect(body?.highlights.length).toBeGreaterThan(0);
    for (const [start, end] of body?.highlights ?? []) expect(body?.snippet.slice(start, end).toLowerCase()).toMatch(/ghi|chú|chu/);
  });

  it("returns nothing for blank queries and hides trashed pages", async () => {
    const { api, control } = await localCore();
    const id = await pageWith(api, control, "Secret plans", "banana");
    expect(await api.searchQuery({ text: "  " })).toEqual([]);
    expect(await api.searchQuery({ text: "banana" })).toHaveLength(1);
    await api.treeTrash({ id });
    expect(await api.searchQuery({ text: "banana" })).toEqual([]);
  });

  it("finds backlinks from page link blocks and inline links", async () => {
    const { api, control } = await localCore();
    const target = await api.treeCreate({ kind: "page", parentId: null, title: "Target" });
    const viaBlock = await api.treeCreate({ kind: "page", parentId: null, title: "Via block" });
    const viaInline = await api.treeCreate({ kind: "page", parentId: null, title: "Via inline" });

    const blockDoc = control.pageDoc(viaBlock) as LoroDoc;
    const list = blockDoc.getMap("doc").get("children") as LoroList;
    const node = list.insertContainer(list.length, new LoroMap());
    node.set("nodeName", "page_link");
    const attrs = node.setContainer("attributes", new LoroMap());
    attrs.set("pageId", target);
    attrs.set("title", "Target");
    node.setContainer("children", new LoroList());
    blockDoc.commit();

    const inlineDoc = control.pageDoc(viaInline) as LoroDoc;
    appendParagraph(inlineDoc, "see Target now");
    const paragraph = (inlineDoc.getMap("doc").get("children") as LoroList).get(1) as LoroMap;
    const text = (paragraph.get("children") as LoroList).get(0) as LoroText;
    inlineDoc.configTextStyle({ link: { expand: "none" } });
    text.mark({ start: 4, end: 10 }, "link", { href: `ne-page:${target}` });
    inlineDoc.commit();

    const links = await api.pageBacklinks({ id: target });
    expect(links.map((link) => link.title).sort()).toEqual(["Via block", "Via inline"]);
  });

  it("forwards a remote update to listeners and merges it into the stored page", async () => {
    const { api, events, control } = await localCore();
    const id = await api.treeCreate({ kind: "page", parentId: null, title: "p" });
    const seen = vi.fn();
    await events.on("page:remote-update", seen);
    const peer = new LoroDoc();
    peer.import((await api.pageOpen({ id })).snapshot);
    const before = peer.version();
    appendParagraph(peer, "from another device");
    const update = peer.export({ mode: "update", from: before });
    control.receiveRemoteUpdate(id, update);
    expect(seen).toHaveBeenCalledWith({ pageId: id, update });
    expect(JSON.stringify(control.pageDoc(id)?.getMap("doc").toJSON())).toContain("from another device");
  });

  it("stores attachments by content hash", async () => {
    const { api } = await localCore();
    const first = await api.attachmentAddBytes({ bytes: new Uint8Array([1, 2, 3]), mime: "image/png" });
    const again = await api.attachmentAddBytes({ bytes: new Uint8Array([1, 2, 3]), mime: "image/png" });
    expect(again.hash).toBe(first.hash);
    expect(await api.attachmentUrl({ hash: first.hash })).toBe(first.url);
    expect((await rejection(api.attachmentUrl({ hash: "nope" }))).code).toBe("not_found");
  });

  it("exports and imports Markdown through the file stand-ins", async () => {
    const saved: [string, string][] = [];
    const file = new File(["first line\n\nsecond line"], "Imported note.md", { type: "text/markdown" });
    const { api, control } = await localCore({
      saveFile: (name: string, content: string) => saved.push([name, content]),
      pickFiles: async () => [file],
    });
    expect(await api.importMarkdown({ parentId: null })).toEqual({ pagesCreated: 1 });
    const [node] = await api.treeSnapshot();
    expect(node?.title).toBe("Imported note");
    expect(JSON.stringify(control.pageDoc(node?.id ?? "")?.getMap("doc").toJSON())).toContain("second line");
    expect(await api.exportMarkdown({ id: null })).toEqual({ pagesWritten: 1 });
    expect(saved[0]?.[1]).toContain("# Imported note");
  });
});

describe("in-memory core: session and sync", () => {
  const signUp = (core: ReturnType<typeof createMemoryCore>, overrides = {}) =>
    core.api.signUp({ serverUrl: "https://notes.example.test", setupCode: MEMORY_SETUP_CODE, username: "ada", password: GOOD_PASSWORD, deviceLabel: "Laptop", ...overrides });

  it("starts empty and becomes local", async () => {
    const core = createMemoryCore();
    expect(await core.api.sessionStatus()).toEqual({ state: "empty" });
    await core.api.workspaceCreateLocal();
    expect((await core.api.sessionStatus()).state).toBe("local");
    expect((await rejection(core.api.workspaceCreateLocal())).code).toBe("exists");
  });

  it("validates sign-up and returns a recovery key", async () => {
    const core = createMemoryCore();
    expect((await rejection(signUp(core, { serverUrl: "notes" }))).code).toBe("bad_server_url");
    expect((await rejection(signUp(core, { setupCode: "wrong" }))).code).toBe("invalid_setup_code");
    expect((await rejection(signUp(core, { password: "short" }))).code).toBe("weak_password");
    const events: CoreEventMap["session:changed"][] = [];
    await core.events.on("session:changed", (status) => events.push(status));
    const { recoveryKey } = await signUp(core);
    expect(recoveryKey).toMatch(/^[A-Z2-9]{4}(-[A-Z2-9]{4}){5}$/);
    expect(events.at(-1)).toMatchObject({ state: "signed_in", username: "ada" });
    expect((await rejection(signUp(core))).code).toBe("username_taken");
  });

  it("signs in, signs out into the password-needed state, and recovers with the key", async () => {
    const core = createMemoryCore();
    const { recoveryKey } = await signUp(core);
    await core.api.signOut();
    expect((await core.api.sessionStatus()).state).toBe("needs_sign_in");
    const args = { serverUrl: "https://notes.example.test", username: "ada", deviceLabel: "Laptop" };
    expect((await rejection(core.api.signIn({ ...args, password: "nope" }))).code).toBe("wrong_password");
    expect((await rejection(core.api.signIn({ ...args, username: "bob", password: GOOD_PASSWORD }))).code).toBe("unknown_user");
    expect((await rejection(core.api.recoverAccount({ ...args, recoveryKey: "AAAA", newPassword: "another long one" }))).code).toBe("bad_recovery_key");
    await core.api.recoverAccount({ ...args, recoveryKey: recoveryKey.toLowerCase(), newPassword: "another long one" });
    expect((await core.api.sessionStatus()).state).toBe("signed_in");
    core.control.expireSession();
    expect((await core.api.sessionStatus()).state).toBe("needs_sign_in");
    await core.api.signIn({ ...args, password: "another long one" });
  });

  it("changes the password and manages device sessions", async () => {
    const core = createMemoryCore();
    await signUp(core);
    expect((await rejection(core.api.changePassword({ currentPassword: "x", newPassword: "whatever long" }))).code).toBe("wrong_password");
    await core.api.changePassword({ currentPassword: GOOD_PASSWORD, newPassword: "whatever long" });
    const devices = await core.api.sessionsList();
    const current = devices.find((device) => device.current);
    const other = devices.find((device) => !device.current);
    expect(current?.deviceLabel).toBe("Laptop");
    expect((await rejection(core.api.sessionsRevoke({ id: current?.id ?? "" }))).code).toBe("current_session");
    await core.api.sessionsRevoke({ id: other?.id ?? "" });
    expect((await core.api.sessionsList()).some((device) => device.id === other?.id)).toBe(false);
  });

  it("walks the sync states: disabled, synced, pending, offline, synced", async () => {
    const core = createMemoryCore({ syncDelayMs: 10_000 });
    const states: string[] = [];
    await core.events.on("sync:status", (status) => states.push(status.state));
    await core.api.workspaceCreateLocal();
    expect((await core.api.syncStatus()).state).toBe("disabled");
    expect((await rejection(core.api.syncNow())).code).toBe("not_signed_in");

    await signUp(core);
    expect(await core.api.syncStatus()).toMatchObject({ state: "synced", pendingUpdates: 0 });

    await core.api.treeCreate({ kind: "page", parentId: null, title: "x" });
    expect(await core.api.syncStatus()).toMatchObject({ state: "pending", pendingUpdates: 1 });

    core.control.setOffline(true);
    expect((await core.api.syncStatus()).state).toBe("offline");
    expect((await rejection(core.api.syncNow())).code).toBe("offline");

    core.control.setOffline(false);
    await core.api.syncNow();
    const done = await core.api.syncStatus();
    expect(done).toMatchObject({ state: "synced", pendingUpdates: 0 });
    expect(done.lastSyncedAt).not.toBeNull();
    expect(states).toEqual(expect.arrayContaining(["disabled", "synced", "pending", "offline"]));
  });
});

describe("in-memory core: vault", () => {
  const unlocked = async (options = {}) => {
    const core = createMemoryCore({ reauthGraceMs: 60_000, ...options });
    await core.api.vaultCreate({ password: GOOD_PASSWORD });
    return core;
  };

  it("moves through uninitialised, unlocked, locked and unlocked again", async () => {
    const core = createMemoryCore();
    const states: string[] = [];
    await core.events.on("vault:state", ({ state }) => states.push(state));
    expect(await core.api.vaultState()).toBe("uninitialised");
    expect((await rejection(core.api.vaultUnlock({ password: GOOD_PASSWORD }))).code).toBe("no_vault");
    expect((await rejection(core.api.vaultCreate({ password: "short" }))).code).toBe("weak_password");
    const { recoveryKey } = await core.api.vaultCreate({ password: GOOD_PASSWORD });
    expect(recoveryKey).toMatch(/^[A-Z2-9]{4}(-[A-Z2-9]{4}){5}$/);
    expect(await core.api.vaultState()).toBe("unlocked");
    await core.api.vaultLock();
    expect((await rejection(core.api.vaultList({}))).code).toBe("vault_locked");
    expect((await rejection(core.api.vaultUnlock({ password: "wrong password!" }))).code).toBe("wrong_password");
    await core.api.vaultUnlock({ password: GOOD_PASSWORD });
    expect(states).toEqual(["unlocked", "locked", "unlocked"]);
    expect((await rejection(core.api.vaultCreate({ password: GOOD_PASSWORD }))).code).toBe("vault_exists");
  });

  it("reports when the vault password equals the account password", async () => {
    const core = createMemoryCore();
    await core.api.signUp({ serverUrl: "https://notes.example.test", setupCode: MEMORY_SETUP_CODE, username: "ada", password: GOOD_PASSWORD, deviceLabel: "L" });
    expect((await core.api.vaultCreate({ password: GOOD_PASSWORD })).sameAsAccountPassword).toBe(true);
    const other = createMemoryCore();
    await other.api.signUp({ serverUrl: "https://notes.example.test", setupCode: MEMORY_SETUP_CODE, username: "ada", password: GOOD_PASSWORD, deviceLabel: "L" });
    expect((await other.api.vaultCreate({ password: "a different long one" })).sameAsAccountPassword).toBe(false);
  });

  it("saves, searches and deletes items without exposing secrets in the list", async () => {
    const { api } = await unlocked();
    const saved = await api.vaultSave({ item: { kind: "login", title: "Mail", username: "ada@example.test", password: "p@ss", urls: ["https://mail.example.test"], tags: ["personal"] } });
    await api.vaultSave({ item: { kind: "api_key", title: "Deploy token", value: "tok_123", tags: ["work"] } });
    const all = await api.vaultList({});
    expect(all.map((item) => item.title)).toEqual(["Deploy token", "Mail"]);
    expect(JSON.stringify(all)).not.toContain("p@ss");
    expect(JSON.stringify(all)).not.toContain("tok_123");
    expect((await api.vaultList({ query: "ada" })).map((item) => item.title)).toEqual(["Mail"]);
    expect((await api.vaultList({ query: "work" })).map((item) => item.title)).toEqual(["Deploy token"]);
    expect((await rejection(api.vaultSave({ item: { kind: "login", title: "  " } }))).code).toBe("title_required");
    await api.vaultDelete({ id: saved.id });
    expect(await api.vaultList({})).toHaveLength(1);
  });

  it("keeps stored secrets when an edit leaves them out", async () => {
    const { api } = await unlocked();
    const item = await api.vaultSave({ item: { kind: "login", title: "Mail", password: "keep-me" } });
    await api.vaultSave({ item: { id: item.id, kind: "login", title: "Mail renamed" } });
    expect(await api.vaultReveal({ id: item.id, field: "password" })).toBe("keep-me");
    await api.vaultSave({ item: { id: item.id, kind: "login", title: "Mail renamed", password: "new" } });
    expect(await api.vaultReveal({ id: item.id, field: "password" })).toBe("new");
  });

  it("asks for the password again once the grace period has passed", async () => {
    const { api } = await unlocked({ reauthGraceMs: 0 });
    const item = await api.vaultSave({ item: { kind: "login", title: "Mail", password: "s3cret" } });
    await new Promise((resolve) => setTimeout(resolve, 2));
    expect((await rejection(api.vaultReveal({ id: item.id, field: "password" }))).code).toBe("reauth_required");
    expect((await rejection(api.vaultReveal({ id: item.id, field: "password", password: "nope nope nope" }))).code).toBe("wrong_password");
    expect(await api.vaultReveal({ id: item.id, field: "password", password: GOOD_PASSWORD })).toBe("s3cret");
    expect((await rejection(api.vaultCopy({ id: item.id, field: "password" }))).code).toBe("reauth_required");
  });

  it("copies with a clear-after delay and never needs re-auth for the username", async () => {
    const { api, control } = await unlocked({ reauthGraceMs: 0, clipboardSeconds: 12 });
    const item = await api.vaultSave({ item: { kind: "login", title: "Mail", username: "ada", password: "s3cret" } });
    await new Promise((resolve) => setTimeout(resolve, 2));
    expect(await api.vaultCopy({ id: item.id, field: "username" })).toEqual({ clearAfterSeconds: 12 });
    expect(control.clipboard()).toBe("ada");
  });

  it("locks by itself after the idle time and when told to", async () => {
    vi.useFakeTimers();
    try {
      const core = await unlocked();
      await core.api.settingsSet({ settings: { vaultIdleLockSeconds: 1 } });
      await core.api.vaultList({});
      await vi.advanceTimersByTimeAsync(1100);
      expect(await core.api.vaultState()).toBe("locked");
      await core.api.vaultUnlock({ password: GOOD_PASSWORD });
      core.control.autoLock();
      expect(await core.api.vaultState()).toBe("locked");
    } finally {
      vi.useRealTimers();
    }
  });

  it("recovers with the key and changes the password", async () => {
    const core = createMemoryCore();
    const { recoveryKey } = await core.api.vaultCreate({ password: GOOD_PASSWORD });
    await core.api.vaultLock();
    expect((await rejection(core.api.vaultRecover({ recoveryKey: "BAD", newPassword: "another long one" }))).code).toBe("bad_recovery_key");
    await core.api.vaultRecover({ recoveryKey, newPassword: "another long one" });
    expect(await core.api.vaultState()).toBe("unlocked");
    expect((await rejection(core.api.vaultChangePassword({ currentPassword: GOOD_PASSWORD, newPassword: "third long one!" }))).code).toBe("wrong_password");
    await core.api.vaultChangePassword({ currentPassword: "another long one", newPassword: "third long one!" });
    await core.api.vaultLock();
    await core.api.vaultUnlock({ password: "third long one!" });
  });

  it("generates passwords that honour the options", async () => {
    const { api } = await unlocked();
    const digitsOnly = await api.vaultGeneratePassword({ mode: "characters", length: 24, lowercase: false, uppercase: false, digits: true, symbols: false });
    expect(digitsOnly.value).toMatch(/^\d{24}$/);
    const clear = await api.vaultGeneratePassword({ mode: "characters", length: 64, excludeAmbiguous: true });
    expect(clear.value).not.toMatch(/[O0Il1]/);
    expect(clear.value).toMatch(/[a-z]/);
    expect(clear.value).toMatch(/[A-Z]/);
    expect(clear.entropyBits).toBeGreaterThan(100);
    const phrase = await api.vaultGeneratePassword({ mode: "passphrase", length: 5 });
    expect(phrase.value.split("-")).toHaveLength(5);
    expect((await rejection(api.vaultGeneratePassword({ mode: "characters", length: 20, lowercase: false, uppercase: false, digits: false }))).code).toBe("no_character_sets");
  });

  it("imports CSV and reports skipped rows", async () => {
    const csv = 'name,login_username,login_password,login_uri,notes\nMail,ada,"p,ss",https://mail.test,"two\nlines"\n,nobody,x,,\nBank,ada,pw,,\n';
    const { api } = await unlocked({ pickFiles: async () => [new File([csv], "export.csv")] });
    const report = await api.vaultImportCsv({ format: "bitwarden" });
    expect(report).toEqual({ imported: 2, skipped: [{ row: 3, reason: "Missing title" }] });
    const mail = (await api.vaultList({})).find((item) => item.title === "Mail");
    expect(mail?.urls).toEqual(["https://mail.test"]);
    expect(await api.vaultReveal({ id: mail?.id ?? "", field: "password" })).toBe("p,ss");
    expect(await api.vaultReveal({ id: mail?.id ?? "", field: "notes" })).toBe("two\nlines");
  });

  it("returns null when the file picker is cancelled", async () => {
    const { api } = await unlocked({ pickFiles: async () => [] });
    expect(await api.vaultImportCsv({ format: "generic" })).toBeNull();
  });

  it("parses quoted CSV fields", () => {
    expect(parseCsv('a,"b ""q"" c",\r\n1,2,3')).toEqual([["a", 'b "q" c', ""], ["1", "2", "3"]]);
  });
});

describe("in-memory core: voice and settings", () => {
  it("needs a model, downloads with progress, records and returns text", async () => {
    const core = createMemoryCore();
    const progress: number[] = [];
    await core.events.on("voice:download-progress", ({ receivedBytes }) => progress.push(receivedBytes));
    expect((await rejection(core.api.voiceStart())).code).toBe("no_model");
    await core.api.voiceDownloadModel({ name: "small-q5" });
    expect(progress.length).toBeGreaterThan(1);
    expect(progress).toEqual([...progress].sort((a, b) => a - b));
    const status = await core.api.voiceStatus();
    expect(status).toMatchObject({ available: true, activeModel: "small-q5" });
    core.control.setNextTranscript("xin chào");
    await core.api.voiceStart();
    expect((await core.api.voiceStatus()).recording).toBe(true);
    expect(await core.api.voiceStop()).toEqual({ text: "xin chào" });
    expect((await rejection(core.api.voiceStop())).code).toBe("not_recording");
  });

  it("can report that transcription is unavailable", async () => {
    expect((await createMemoryCore({ voiceAvailable: false }).api.voiceStatus()).available).toBe(false);
  });

  it("merges settings patches", async () => {
    const { api } = createMemoryCore();
    const next = await api.settingsSet({ settings: { theme: "dark" } });
    expect(next.theme).toBe("dark");
    expect((await api.settingsGet()).voiceLanguage).toBe("auto");
  });
});
