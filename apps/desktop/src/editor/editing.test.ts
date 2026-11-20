import { afterEach, describe, expect, it, vi } from "vitest";
import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import { TextSelection } from "prosemirror-state";
import { createTestEditor, settle, type TestEditor } from "../test/editor-harness.ts";
import { createPageDoc } from "./binding.ts";
import {
  applyBlockSuggestion,
  applyPageSuggestion,
  clearFind,
  findMatches,
  findSuggestion,
  getFindState,
  insertChoice,
  pageSchema,
  setFindQuery,
  stepFind,
  type SuggestionState,
} from "./index.ts";

let current: TestEditor | undefined;
afterEach(() => current?.destroy());

const make = async (options?: Parameters<typeof createTestEditor>[0]) => (current = await createTestEditor(options));

describe("Markdown input rules (product spec section 5)", () => {
  it("turns typed prefixes into blocks", async () => {
    const t = await make();
    const cases: [string, string, (t: TestEditor) => void][] = [
      ["# ", "heading", (x) => expect(x.view.state.doc.firstChild?.attrs.level).toBe(1)],
      ["### ", "heading", (x) => expect(x.view.state.doc.firstChild?.attrs.level).toBe(3)],
      ["- ", "bullet_list", () => undefined],
      ["1. ", "ordered_list", () => undefined],
      ["[] ", "todo_list", () => undefined],
      ["> ", "quote", () => undefined],
      ["```", "code_block", () => undefined],
    ];
    for (const [typed, kind, extra] of cases) {
      t.setText("");
      t.type(typed);
      expect(t.topKinds()[0], typed).toBe(kind);
      extra(t);
    }
  });

  it("treats > as a quote, never as a toggle", async () => {
    const t = await make();
    t.setText("");
    t.type("> ");
    expect(t.topKinds()).toEqual(["quote"]);
    expect(t.topKinds()).not.toContain("toggle");
  });

  it("turns --- into a divider followed by an empty paragraph", async () => {
    const t = await make();
    t.setText("");
    t.type("---");
    expect(t.topKinds()).toEqual(["divider", "paragraph"]);
  });

  it("applies inline marks", async () => {
    const t = await make();
    for (const [typed, mark] of [
      ["**bold**", "bold"],
      ["*soft*", "italic"],
      ["`mono`", "code"],
      ["~~gone~~", "strike"],
    ] as const) {
      t.setText("");
      t.type(typed);
      const marks = t.view.state.doc.firstChild?.firstChild?.marks.map((m) => m.type.name);
      expect(marks, typed).toContain(mark);
    }
  });

  it("splits list items with Enter and nests with Tab", async () => {
    const t = await make();
    t.setText("");
    t.type("- one");
    expect(t.press("Enter")).toBe(true);
    t.type("two");
    expect(t.view.state.doc.firstChild?.childCount).toBe(2);
    expect(t.press("Tab")).toBe(true);
    expect(t.view.state.doc.firstChild?.firstChild?.childCount).toBe(2);
  });

  it("keeps Shift+Enter inside the block as a line break", async () => {
    const t = await make();
    t.setText("ab", 1);
    expect(t.press("Enter", { shiftKey: true })).toBe(true);
    expect(t.topKinds()).toEqual(["paragraph"]);
    expect(t.view.state.doc.firstChild?.childCount).toBe(3);
  });
});

describe("suggestion triggers", () => {
  const stateAfter = async (text: string) => {
    const t = await make();
    t.setText("");
    t.type(text);
    return findSuggestion(t.view.state);
  };

  it("opens the slash menu at the start of a block or after a space", async () => {
    expect(await stateAfter("/hea")).toMatchObject({ trigger: "slash", query: "hea" });
    expect(await stateAfter("note /")).toMatchObject({ trigger: "slash", query: "" });
  });

  it("does not open for slashes inside words or URLs", async () => {
    expect(await stateAfter("a/b")).toBeNull();
    expect(await stateAfter("see https://x.io/")).toBeNull();
  });

  it("opens the page picker for [[ and @", async () => {
    expect(await stateAfter("[[gar")).toMatchObject({ trigger: "page", query: "gar" });
    expect(await stateAfter("hi @tri")).toMatchObject({ trigger: "page", query: "tri" });
  });

  it("ignores email addresses and code blocks", async () => {
    expect(await stateAfter("mail me@x.io")).toBeNull();
    const t = await make();
    t.setText("");
    t.type("```");
    t.type("/todo");
    expect(findSuggestion(t.view.state)).toBeNull();
  });

  it("reports to the host as the cursor moves and closes when it no longer matches", async () => {
    const seen: (SuggestionState | null)[] = [];
    const t = await make({ host: { onSuggest: (state) => seen.push(state) } });
    t.setText("");
    t.type("/to");
    expect(seen.at(-1)).toMatchObject({ trigger: "slash", query: "to" });
    t.type(" ");
    expect(seen.at(-1)).toBeNull();
  });

  it("stays closed after dismissal until the trigger changes", async () => {
    let state: SuggestionState | null = null;
    const t = await make({ host: { onSuggest: (next) => (state = next) } });
    t.setText("");
    t.type("/to");
    expect(state).not.toBeNull();
    t.editor.dismissSuggestion();
    expect(state).toBeNull();
    t.type("g");
    expect(state).toBeNull();
  });

  it("hands keys to the host only while a menu is open", async () => {
    const keys: string[] = [];
    const t = await make({ host: { onSuggestKey: (event) => (keys.push(event.key), true) } });
    t.setText("plain");
    expect(t.press("ArrowDown")).toBe(false);
    t.setText("");
    t.type("/");
    expect(t.press("ArrowDown")).toBe(true);
    expect(keys).toEqual(["ArrowDown"]);
  });
});

describe("inserting from a suggestion", () => {
  it("replaces /query with the chosen block", async () => {
    const t = await make();
    t.setText("");
    t.type("/todo");
    const match = findSuggestion(t.view.state);
    expect(match).not.toBeNull();
    applyBlockSuggestion(match as NonNullable<typeof match>, "todo_list")(t.view.state, t.view.dispatch, t.view);
    expect(t.topKinds()).toEqual(["todo_list"]);
    expect(t.view.state.doc.textContent).toBe("");
  });

  it("inserts a block after a non-empty block", async () => {
    const t = await make();
    t.setText("keep me");
    t.type(" /div");
    const match = findSuggestion(t.view.state);
    applyBlockSuggestion(match as NonNullable<typeof match>, "divider")(t.view.state, t.view.dispatch, t.view);
    expect(t.topKinds().slice(0, 2)).toEqual(["paragraph", "divider"]);
    expect(t.view.state.doc.firstChild?.textContent).toBe("keep me ");
  });

  it("inserts an inline page link carrying the page id", async () => {
    const t = await make();
    t.setText("see ");
    t.type("[[gar");
    const match = findSuggestion(t.view.state);
    applyPageSuggestion(match as NonNullable<typeof match>, { id: "page-1", title: "Garden plan" })(t.view.state, t.view.dispatch);
    const paragraph = t.view.state.doc.firstChild;
    expect(paragraph?.textContent).toBe("see Garden plan ");
    const linked = paragraph?.child(1);
    expect(linked?.text).toBe("Garden plan");
    expect(linked?.marks[0]?.attrs.href).toBe("ne-page:page-1");
    expect(paragraph?.lastChild?.marks).toHaveLength(0);
  });
});

describe("block insertion of every kind", () => {
  it("builds all 15 slash kinds into a valid document", async () => {
    const { BLOCK_CHOICES } = await import("./index.ts");
    expect(BLOCK_CHOICES).toHaveLength(15);
    const t = await make();
    for (const choice of BLOCK_CHOICES) {
      t.setText("");
      expect(insertChoice(choice, { pageId: "p", title: "T", src: "attachment:x" })(t.view.state, t.view.dispatch), choice).toBe(true);
      t.view.state.doc.check();
    }
  });
});

describe("local and remote updates", () => {
  it("emits Loro updates that rebuild the same text elsewhere", async () => {
    const t = await make();
    t.setText("");
    t.type("hello");
    await settle();
    expect(t.updates.length).toBeGreaterThan(0);
    const other = new LoroDoc();
    for (const update of t.updates) other.import(update);
    const text = JSON.stringify(other.getMap("doc").toJSON());
    expect(text).toContain("hello");
  });

  it("applies a remote update without moving the caret off its text", async () => {
    const t = await make();
    t.setText("Hello world", 5);
    await settle();
    const peer = new LoroDoc();
    peer.import(t.doc.export({ mode: "snapshot" }));
    const root = peer.getMap("doc").get("children") as LoroList;
    const paragraph = root.get(0) as LoroMap;
    const text = (paragraph.get("children") as LoroList).get(0) as LoroText;
    text.insert(0, "XYZ ");
    peer.commit();
    t.editor.importRemote(peer.export({ mode: "update", from: t.doc.version() }));
    await settle(60);

    expect(t.view.state.doc.textContent).toBe("XYZ Hello world");
    // The caret stays after "Hello": it moved right with the inserted text.
    expect(t.view.state.selection.from).toBe(1 + "XYZ Hello".length);
  });

  it("keeps the caret and the typed text when a remote update arrives mid-typing", async () => {
    const t = await make();
    t.setText("abc def", 3);
    await settle();
    t.type("X");
    const peer = new LoroDoc();
    peer.import(t.doc.export({ mode: "snapshot" }));
    const paragraph = (peer.getMap("doc").get("children") as LoroList).get(0) as LoroMap;
    const text = (paragraph.get("children") as LoroList).get(0) as LoroText;
    text.insert(text.length, " tail");
    peer.commit();
    t.editor.importRemote(peer.export({ mode: "update", from: t.doc.version() }));
    t.type("Y");
    await settle(60);
    expect(t.view.state.doc.textContent).toBe("abcXY def tail");
    expect(t.view.state.selection.from).toBe(1 + "abcXY".length);
  });

  it("batches updates that arrive together", async () => {
    const t = await make();
    t.setText("a");
    await settle();
    const peer = new LoroDoc();
    peer.import(t.doc.export({ mode: "snapshot" }));
    const text = ((peer.getMap("doc").get("children") as LoroList).get(0) as LoroMap).get("children") as LoroList;
    const run = text.get(0) as LoroText;
    const spy = vi.spyOn(t.doc, "importBatch");
    const before = t.doc.version();
    run.insert(1, "b");
    peer.commit();
    const first = peer.export({ mode: "update", from: before });
    const mid = peer.version();
    run.insert(2, "c");
    peer.commit();
    const second = peer.export({ mode: "update", from: mid });
    t.editor.importRemote(first);
    t.editor.importRemote(second);
    await settle(60);
    expect(spy).toHaveBeenCalledTimes(1);
    expect(t.view.state.doc.textContent).toBe("abc");
  });

  it("undo only reverts this device's own edits", async () => {
    // Open a page whose text already exists, so only the typing below is local history.
    const seed = await make();
    seed.setText("base");
    await settle();
    const doc = createPageDoc();
    doc.import(seed.doc.export({ mode: "snapshot" }));
    const t = await createTestEditor({ doc });
    t.view.dispatch(t.view.state.tr.setSelection(TextSelection.create(t.view.state.doc, 1 + 4)));
    t.type(" mine");
    await settle();

    const peer = new LoroDoc();
    peer.import(t.doc.export({ mode: "snapshot" }));
    const paragraph = (peer.getMap("doc").get("children") as LoroList).get(0) as LoroMap;
    const run = (paragraph.get("children") as LoroList).get(0) as LoroText;
    run.insert(0, "theirs ");
    peer.commit();
    t.editor.importRemote(peer.export({ mode: "update", from: t.doc.version() }));
    await settle(60);
    expect(t.view.state.doc.textContent).toBe("theirs base mine");

    t.press("z", { ctrlKey: true });
    await settle(60);
    expect(t.view.state.doc.textContent).toBe("theirs base");
    t.destroy();
  });

  it("opens a snapshot produced elsewhere", async () => {
    const source = await make();
    source.setText("shared text");
    await settle();
    const snapshot = source.doc.export({ mode: "snapshot" });
    const doc = createPageDoc();
    doc.import(snapshot);
    const second = await createTestEditor({ doc });
    expect(second.view.state.doc.textContent).toBe("shared text");
    second.destroy();
  });
});

describe("find in page", () => {
  it("matches ignoring case and accents and cycles through matches", async () => {
    const t = await make();
    t.setText("Ghi chú đầu tiên. ghi chu thứ hai.");
    expect(findMatches(t.view.state.doc, "ghi chu")).toHaveLength(2);
    setFindQuery(t.view, "GHI CHU");
    expect(getFindState(t.view.state)).toMatchObject({ current: 0 });
    expect(getFindState(t.view.state).matches).toHaveLength(2);
    const first = t.view.state.selection.from;
    stepFind(t.view, 1);
    expect(t.view.state.selection.from).toBeGreaterThan(first);
    stepFind(t.view, 1);
    expect(t.view.state.selection.from).toBe(first);
    stepFind(t.view, -1);
    expect(getFindState(t.view.state).current).toBe(1);
    clearFind(t.view);
    expect(getFindState(t.view.state).matches).toHaveLength(0);
  });

  it("reports no matches for text that is not there", async () => {
    const t = await make();
    t.setText("nothing here");
    setFindQuery(t.view, "zebra");
    expect(getFindState(t.view.state)).toMatchObject({ matches: [], current: -1 });
  });
});

describe("schema", () => {
  it("keeps the persisted node names of the page document: the 13 block kinds and the synced-block placeholder", () => {
    expect(Object.keys(pageSchema.nodes).sort()).toEqual(
      [
        "bullet_list", "callout", "child_page", "code_block", "divider", "doc", "hard_break", "heading", "image",
        "list_item", "ordered_list", "page_link", "paragraph", "quote", "synced_block", "text", "todo_item", "todo_list", "toggle",
      ].sort(),
    );
  });

  it("selects through TextSelection helpers used by the UI", async () => {
    const t = await make();
    t.setText("abc", 1);
    expect(t.view.state.selection).toBeInstanceOf(TextSelection);
  });
});
