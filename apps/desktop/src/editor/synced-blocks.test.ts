import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import { TextSelection } from "prosemirror-state";
import { describe, expect, it, vi } from "vitest";
import { waitFor } from "@solidjs/testing-library";
import type { Core } from "../api/index.ts";
import { createMemoryCore } from "../api/memory.ts";
import { createSyncedHub } from "../state/synced-hub.ts";
import { createBlock, createSyncedBlock, insertSyncedBlock } from "./blocks.ts";
import { createPageDoc } from "./binding.ts";
import { createPageEditor, type PageEditor } from "./editor.ts";
import { pageSchema } from "./schema.ts";
import { syncedReference } from "./page-links.ts";
import { settle } from "../test/editor-harness.ts";

async function setup() {
  const memory = createMemoryCore({ syncDelayMs: 0 });
  await memory.api.workspaceCreateLocal();
  const core: Core = { api: memory.api, events: memory.events, preview: false };
  const hub = createSyncedHub(core, (error) => {
    throw error;
  });
  const page = async (title: string, ...words: string[]) => {
    const id = await memory.api.treeCreate({ kind: "page", parentId: null, title });
    const doc = memory.control.pageDoc(id) as LoroDoc;
    const children = doc.getMap("doc").get("children") as LoroList;
    children.delete(0, children.length);
    for (const word of words) {
      const paragraph = children.insertContainer(children.length, new LoroMap());
      paragraph.set("nodeName", "paragraph");
      paragraph.setContainer("attributes", new LoroMap());
      paragraph.setContainer("children", new LoroList()).insertContainer(0, new LoroText()).insert(0, word);
    }
    doc.commit();
    return id;
  };
  const open = async (id: string, onError?: (message: string) => void): Promise<PageEditor> => {
    const doc = createPageDoc();
    doc.import((await memory.api.pageOpen({ id })).snapshot);
    const mount = document.createElement("div");
    document.body.appendChild(mount);
    const editor = createPageEditor(mount, { doc, host: { synced: hub, onError } });
    await memory.events.on("page:remote-update", ({ pageId, update }) => pageId === id && editor.importRemote(update));
    return editor;
  };
  return { memory, hub, page, open };
}

/** The editor inside the first synced block, once the binding has drawn the shared content. */
const nestedOf = async (editor: PageEditor): Promise<PageEditor> => {
  const nested = await waitFor(() => {
    const [found] = editor.nestedEditors();
    if (!found) throw new Error("nested editor not open");
    return found;
  });
  await settle();
  return nested;
};

describe("synced blocks in the editor", () => {
  it("shows shared content in place, marks it as synced and counts the pages that show it", async () => {
    const { memory, page, open } = await setup();
    const a = await page("A", "intro", "shared words");
    const b = await page("B", "other");
    const { syncId } = await memory.api.syncedCreate({ pageId: a, start: 1, end: 1 });
    await memory.api.syncedInsert({ pageId: b, parentPath: [], index: 1, syncId });
    const editor = await open(a);
    const nested = await nestedOf(editor);
    expect(nested.view.state.doc.textContent).toBe("shared words");
    const frame = editor.view.dom.querySelector(".ne-synced") as HTMLElement;
    expect(frame.getAttribute("aria-label")).toBe("Synced block");
    await waitFor(() => expect(frame.querySelector(".ne-synced-count")?.textContent).toBe("Shown on 2 pages"));
    expect(frame.dataset.state).toBe("ready");
    editor.destroy();
    expect(editor.nestedEditors()).toHaveLength(0);
  });

  it("carries an edit made inside a synced block to the core and to the same block on a second open page", async () => {
    const { memory, page, open } = await setup();
    const a = await page("A", "shared words");
    const b = await page("B", "other");
    const { syncId } = await memory.api.syncedCreate({ pageId: a, start: 0, end: 0 });
    await memory.api.syncedInsert({ pageId: b, parentPath: [], index: 1, syncId });
    const first = await open(a);
    const second = await open(b);
    const inFirst = await nestedOf(first);
    const inSecond = await nestedOf(second);

    inFirst.view.dispatch(inFirst.view.state.tr.insertText("Edited: ", 1));
    await waitFor(() => expect(inSecond.view.state.doc.textContent).toBe("Edited: shared words"));
    await waitFor(() => expect(JSON.stringify((memory.control.syncedDoc(syncId) as LoroDoc).getMap("doc").toJSON())).toContain("Edited: shared words"));

    // The outer pages are untouched: they hold only the placeholder.
    expect(first.view.state.doc.child(0).type.name).toBe("synced_block");
    expect(second.view.state.doc.childCount).toBe(2);
    first.destroy();
    second.destroy();
  });

  it("applies an update that arrives from another device to every open view", async () => {
    const { memory, page, open } = await setup();
    const a = await page("A", "shared words");
    const { syncId } = await memory.api.syncedCreate({ pageId: a, start: 0, end: 0 });
    const editor = await open(a);
    const nested = await nestedOf(editor);

    const peer = new LoroDoc();
    peer.import((await memory.api.syncedOpen({ syncId })).snapshot);
    const before = peer.version();
    const paragraph = (peer.getMap("doc").get("children") as LoroList).get(0) as LoroMap;
    ((paragraph.get("children") as LoroList).get(0) as LoroText).insert(0, "Remote: ");
    peer.commit();
    memory.control.receiveRemoteSyncedUpdate(syncId, peer.export({ mode: "update", from: before }));
    await waitFor(() => expect(nested.view.state.doc.textContent).toBe("Remote: shared words"));
    editor.destroy();
  });

  it("waits for content that has not arrived and shows it once it does", async () => {
    const { memory, page, open, hub } = await setup();
    const a = await page("A", "shared words");
    const b = await page("B", "x");
    const { syncId } = await memory.api.syncedCreate({ pageId: a, start: 0, end: 0 });
    const original = memory.api.syncedOpen;
    let arrived = false;
    memory.api.syncedOpen = (args) => (arrived ? original(args) : Promise.reject({ code: "not_found", message: "not yet" }));
    await memory.api.syncedInsert({ pageId: b, parentPath: [], index: 1, syncId });
    const editor = await open(b);
    const frame = await waitFor(() => {
      const found = editor.view.dom.querySelector(".ne-synced") as HTMLElement | null;
      if (found?.dataset.state !== "missing") throw new Error("not missing yet");
      return found;
    });
    expect(frame.textContent).toContain("has not arrived");
    expect(editor.nestedEditors()).toHaveLength(0);
    arrived = true;
    // A tree change makes the page refresh its blocks, which retries.
    editor.refreshLinks();
    const nested = await nestedOf(editor);
    expect(nested.view.state.doc.textContent).toBe("shared words");
    expect(hub).toBeDefined();
    editor.destroy();
  });

  it("refuses to put a synced block inside shared content, before the core is called", async () => {
    const { memory, page, open } = await setup();
    const a = await page("A", "shared words");
    const { syncId } = await memory.api.syncedCreate({ pageId: a, start: 0, end: 0 });
    const onError = vi.fn();
    const editor = await open(a, onError);
    const nested = await nestedOf(editor);
    const before = nested.view.state.doc;
    nested.view.dispatch(nested.view.state.tr.insert(before.content.size, createSyncedBlock("0@other")));
    expect(nested.view.state.doc.eq(before)).toBe(true);
    expect(nested.view.state.doc.textContent).toBe("shared words");
    expect(onError).toHaveBeenCalled();
    // Pasting a reference into the nested editor is swallowed too.
    const paste = (view: PageEditor["view"], text: string) => {
      const event = { clipboardData: { getData: (type: string) => (type === "text/plain" ? text : ""), files: [] }, preventDefault() {} } as unknown as ClipboardEvent;
      return Boolean(view.someProp("handlePaste", (f) => f(view, event, null as never)));
    };
    expect(paste(nested.view, syncedReference("0@other"))).toBe(true);
    expect(nested.view.state.doc.eq(before)).toBe(true);
    // In a page the same paste places the block.
    const mount = document.createElement("div");
    const plain = createPageEditor(mount, { doc: createPageDoc() });
    await settle();
    expect(paste(plain.view, syncedReference(syncId))).toBe(true);
    const kinds: string[] = [];
    plain.view.state.doc.forEach((node) => kinds.push(node.type.name));
    expect(kinds).toContain("synced_block");
    expect(plain.view.state.doc.child(kinds.indexOf("synced_block")).attrs.syncId).toBe(syncId);
    editor.destroy();
    plain.destroy();
  });

  it("uses the placeholder as a plain atom block that survives turn-into and insertion helpers", () => {
    const node = createSyncedBlock("0@abc");
    expect(node.type).toBe(pageSchema.nodes.synced_block);
    expect(node.isAtom).toBe(true);
    expect(node.childCount).toBe(0);
    expect(createBlock("paragraph").childCount).toBe(0);
    const doc = pageSchema.node("doc", null, [pageSchema.node("paragraph")]);
    expect(doc.check()).toBeUndefined();
    expect(TextSelection.atStart(doc).from).toBe(1);
    expect(typeof insertSyncedBlock("x")).toBe("function");
  });
});
