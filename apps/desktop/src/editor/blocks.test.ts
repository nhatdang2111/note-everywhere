import { afterEach, describe, expect, it, vi } from "vitest";
import { NodeSelection, TextSelection, type Command } from "prosemirror-state";
import { DOMParser as ProseDomParser, type Node as PmNode } from "prosemirror-model";
import { createTestEditor, settle, type TestEditor } from "../test/editor-harness.ts";
import {
  blockRange,
  canTurnInto,
  deleteBlocks,
  duplicateBlocks,
  insertChoice,
  looksLikeMarkdown,
  markdownToNodes,
  moveBlocks,
  moveBlocksBy,
  pageSchema,
  setLink,
  removeLink,
  turnInto,
  type EditorHost,
} from "./index.ts";

const { nodes, marks } = pageSchema;
let current: TestEditor | undefined;
afterEach(() => current?.destroy());
const make = async (host?: EditorHost) => (current = await createTestEditor({ host }));

/** Replace the document with the given blocks and select the start of block `at`. */
function load(t: TestEditor, blocks: PmNode[], at = 0) {
  t.view.dispatch(t.view.state.tr.replaceWith(0, t.view.state.doc.content.size, blocks));
  let pos = 0;
  for (let i = 0; i < at; i += 1) pos += t.view.state.doc.child(i).nodeSize;
  t.view.dispatch(t.view.state.tr.setSelection(TextSelection.near(t.view.state.doc.resolve(pos + 1))));
}

const p = (text: string, ...extra: Parameters<typeof pageSchema.text>[1][]) => nodes.paragraph.create(null, text ? pageSchema.text(text, extra[0]) : undefined);
const texts = (t: TestEditor) => {
  const out: string[] = [];
  t.view.state.doc.forEach((node) => out.push(node.textContent));
  return out;
};
const run = (command: Command, t: TestEditor) => command(t.view.state, t.view.dispatch);

describe("turn into", () => {
  it("keeps text and marks when a paragraph becomes a heading", async () => {
    const t = await make();
    load(t, [nodes.paragraph.create(null, [pageSchema.text("Bold ", [marks.bold.create()]), pageSchema.text("plain")])]);
    expect(run(turnInto("heading2"), t)).toBe(true);
    const heading = t.view.state.doc.firstChild as PmNode;
    expect(heading.type.name).toBe("heading");
    expect(heading.attrs.level).toBe(2);
    expect(heading.textContent).toBe("Bold plain");
    expect(heading.firstChild?.marks[0]?.type.name).toBe("bold");
  });

  it("remembers a checked to-do across heading and back", async () => {
    const t = await make();
    load(t, [nodes.todo_list.create(null, nodes.todo_item.create({ checked: true }, p("Ship it")))]);
    run(turnInto("heading1"), t);
    expect(t.topKinds()).toEqual(["heading"]);
    expect(texts(t)).toEqual(["Ship it"]);
    run(turnInto("todo_list"), t);
    const item = t.view.state.doc.firstChild?.firstChild as PmNode;
    expect(item.type.name).toBe("todo_item");
    expect(item.attrs.checked).toBe(true);
    expect(item.textContent).toBe("Ship it");
  });

  it("remembers the heading level and a toggle's open state", async () => {
    const t = await make();
    load(t, [nodes.heading.create({ level: 3 }, pageSchema.text("Title"))]);
    run(turnInto("paragraph"), t);
    run(turnInto("heading1"), t);
    run(turnInto("paragraph"), t);
    run(turnInto("toggle"), t);
    expect(t.topKinds()).toEqual(["toggle"]);
    const toggle = t.view.state.doc.firstChild as PmNode;
    expect(toggle.attrs.open).toBe(true);
    t.view.dispatch(t.view.state.tr.setNodeAttribute(0, "open", false));
    run(turnInto("callout"), t);
    run(turnInto("toggle"), t);
    expect((t.view.state.doc.firstChild as PmNode).attrs.open).toBe(false);
  });

  it("turns a list into one paragraph per item and keeps nested lists", async () => {
    const t = await make();
    const nested = nodes.bullet_list.create(null, nodes.list_item.create(null, p("child")));
    load(t, [nodes.bullet_list.create(null, [nodes.list_item.create(null, [p("one"), nested]), nodes.list_item.create(null, p("two"))])]);
    run(turnInto("paragraph"), t);
    expect(t.topKinds()).toEqual(["paragraph", "bullet_list", "paragraph"]);
    expect(texts(t)).toEqual(["one", "child", "two"]);
  });

  it("merges several selected blocks into one list, one item each", async () => {
    const t = await make();
    load(t, [p("a"), p("b"), p("c")]);
    t.view.dispatch(t.view.state.tr.setSelection(TextSelection.create(t.view.state.doc, 1, 5)));
    expect(blockRange(t.view.state)).toEqual({ start: 0, end: 1 });
    run(turnInto("bullet_list"), t);
    expect(t.topKinds()).toEqual(["bullet_list", "paragraph"]);
    expect(t.view.state.doc.firstChild?.childCount).toBe(2);
    expect(texts(t)).toEqual(["ab", "c"]);
  });

  it("turns quote content into callout without losing blocks", async () => {
    const t = await make();
    load(t, [nodes.quote.create(null, [p("first"), nodes.heading.create({ level: 2 }, pageSchema.text("second"))])]);
    run(turnInto("callout"), t);
    const callout = t.view.state.doc.firstChild as PmNode;
    expect(callout.type.name).toBe("callout");
    expect(callout.childCount).toBe(2);
    expect(callout.textContent).toBe("firstsecond");
  });

  it("converts a code block, keeping its lines", async () => {
    const t = await make();
    load(t, [nodes.code_block.create({ language: "rust" }, pageSchema.text("line1\nline2"))]);
    run(turnInto("paragraph"), t);
    expect(t.view.state.doc.firstChild?.textContent).toBe("line1line2");
    expect(t.view.state.doc.firstChild?.childCount).toBe(3);
    run(turnInto("code_block"), t);
    const code = t.view.state.doc.firstChild as PmNode;
    expect(code.textContent).toBe("line1\nline2");
    expect(code.attrs.language).toBe("rust");
  });

  it("keeps the caret in the same words", async () => {
    const t = await make();
    load(t, [p("hello world")]);
    t.view.dispatch(t.view.state.tr.setSelection(TextSelection.create(t.view.state.doc, 7)));
    run(turnInto("heading2"), t);
    expect(t.view.state.selection.from).toBe(7);
    run(turnInto("bullet_list"), t);
    expect(t.view.state.doc.textBetween(0, t.view.state.selection.from)).toContain("hello ");
  });

  it("refuses blocks that are not text", async () => {
    const t = await make();
    load(t, [nodes.divider.create(), p("x")]);
    t.view.dispatch(t.view.state.tr.setSelection(NodeSelection.create(t.view.state.doc, 0)));
    expect(canTurnInto(t.view.state)).toBe(false);
    expect(run(turnInto("paragraph"), t)).toBe(false);
  });
});

describe("moving, duplicating and deleting blocks", () => {
  it("moves a block down and up, keeping content", async () => {
    const t = await make();
    load(t, [p("a"), p("b"), p("c")]);
    expect(moveBlocks({ start: 0, end: 0 }, 3)(t.view.state, t.view.dispatch)).toBe(true);
    expect(texts(t)).toEqual(["b", "c", "a"]);
    expect(moveBlocks({ start: 2, end: 2 }, 0)(t.view.state, t.view.dispatch)).toBe(true);
    expect(texts(t)).toEqual(["a", "b", "c"]);
  });

  it("moves a range together", async () => {
    const t = await make();
    load(t, [p("a"), p("b"), p("c"), p("d")]);
    moveBlocks({ start: 0, end: 1 }, 4)(t.view.state, t.view.dispatch);
    expect(texts(t)).toEqual(["c", "d", "a", "b"]);
  });

  it("does nothing when the drop would not change the order", async () => {
    const t = await make();
    load(t, [p("a"), p("b")]);
    expect(moveBlocks({ start: 0, end: 0 }, 0)(t.view.state)).toBe(false);
    expect(moveBlocks({ start: 0, end: 0 }, 1)(t.view.state)).toBe(false);
  });

  it("moves one step with the keyboard commands and stops at the ends", async () => {
    const t = await make();
    load(t, [p("a"), p("b")], 1);
    expect(moveBlocksBy(-1)(t.view.state, t.view.dispatch)).toBe(true);
    expect(texts(t)).toEqual(["b", "a"]);
    expect(moveBlocksBy(-1)(t.view.state, t.view.dispatch)).toBe(false);
  });

  it("carries remembered attributes along with a moved block", async () => {
    const t = await make();
    load(t, [nodes.todo_list.create(null, nodes.todo_item.create({ checked: true }, p("task"))), p("other")]);
    run(turnInto("paragraph"), t);
    moveBlocks({ start: 0, end: 0 }, 2)(t.view.state, t.view.dispatch);
    expect(texts(t)).toEqual(["other", "task"]);
    t.view.dispatch(t.view.state.tr.setSelection(TextSelection.near(t.view.state.doc.resolve(t.view.state.doc.child(0).nodeSize + 1))));
    run(turnInto("todo_list"), t);
    expect((t.view.state.doc.child(1).firstChild as PmNode).attrs.checked).toBe(true);
  });

  it("duplicates and deletes the selected blocks", async () => {
    const t = await make();
    load(t, [p("a"), p("b")]);
    duplicateBlocks(t.view.state, t.view.dispatch);
    expect(texts(t)).toEqual(["a", "a", "b"]);
    deleteBlocks(t.view.state, t.view.dispatch);
    expect(texts(t)).toEqual(["a", "b"]);
  });

  it("leaves an empty paragraph when everything is deleted", async () => {
    const t = await make();
    load(t, [p("only")]);
    deleteBlocks(t.view.state, t.view.dispatch);
    expect(t.topKinds()).toEqual(["paragraph"]);
    expect(texts(t)).toEqual([""]);
  });
});

describe("links", () => {
  it("sets and removes a link on the selection", async () => {
    const t = await make();
    load(t, [p("click here")]);
    t.view.dispatch(t.view.state.tr.setSelection(TextSelection.create(t.view.state.doc, 1, 6)));
    expect(setLink("https://example.com")(t.view.state, t.view.dispatch)).toBe(true);
    expect(t.view.state.doc.firstChild?.firstChild?.marks[0]?.attrs.href).toBe("https://example.com");
    t.view.dispatch(t.view.state.tr.setSelection(TextSelection.create(t.view.state.doc, 3)));
    expect(removeLink(t.view.state, t.view.dispatch)).toBe(true);
    expect(t.view.state.doc.firstChild?.firstChild?.marks).toHaveLength(0);
  });

  it("needs a selection or a link under the cursor", async () => {
    const t = await make();
    load(t, [p("plain")]);
    expect(setLink("https://example.com")(t.view.state)).toBe(false);
  });
});

describe("Markdown paste", () => {
  it("parses blocks and inline marks", () => {
    const blocks = markdownToNodes("# Title\n\nSome **bold**, *soft*, `code` and [a link](https://x.io).\n\n- one\n- two\n  - nested\n\n1. first\n2. second\n\n- [ ] open\n- [x] done\n\n> quoted\n\n---\n\n```ts\nconst a = 1;\n```");
    expect(blocks.map((b) => b.type.name)).toEqual(["heading", "paragraph", "bullet_list", "ordered_list", "todo_list", "quote", "divider", "code_block"]);
    const para = blocks[1] as PmNode;
    const markNames = new Set<string>();
    para.forEach((child) => child.marks.forEach((m) => markNames.add(m.type.name)));
    expect([...markNames].sort()).toEqual(["bold", "code", "italic", "link"]);
    const bullets = blocks[2] as PmNode;
    expect(bullets.childCount).toBe(2);
    expect(bullets.child(1).childCount).toBe(2);
    const todos = blocks[4] as PmNode;
    expect([todos.child(0).attrs.checked, todos.child(1).attrs.checked]).toEqual([false, true]);
    expect((blocks[7] as PmNode).attrs.language).toBe("ts");
    expect((blocks[7] as PmNode).textContent).toBe("const a = 1;");
  });

  it("caps heading depth at 3 and keeps unknown syntax as text", () => {
    expect((markdownToNodes("###### deep")[0] as PmNode).attrs.level).toBe(3);
    expect(markdownToNodes("a | b\n--|--")[0]?.type.name).toBe("paragraph");
  });

  it("detects Markdown but not plain prose", () => {
    expect(looksLikeMarkdown("# Heading")).toBe(true);
    expect(looksLikeMarkdown("some **bold** text")).toBe(true);
    expect(looksLikeMarkdown("- item")).toBe(true);
    expect(looksLikeMarkdown("just a sentence, nothing special.")).toBe(false);
    expect(looksLikeMarkdown("price is 3 - 2")).toBe(false);
  });

  const paste = (t: TestEditor, data: Record<string, string>, files: File[] = []) => {
    const event = { clipboardData: { files, getData: (type: string) => data[type] ?? "" }, preventDefault: () => undefined } as unknown as ClipboardEvent;
    return Boolean(t.view.someProp("handlePaste", (f) => f(t.view, event, null as never)));
  };

  it("pastes Markdown text as blocks", async () => {
    const t = await make();
    t.setText("");
    expect(paste(t, { "text/plain": "# Plan\n\n- a\n- b" })).toBe(true);
    expect(t.topKinds().slice(0, 2)).toEqual(["heading", "bullet_list"]);
  });

  it("leaves plain text and HTML to the default paste", async () => {
    const t = await make();
    t.setText("");
    expect(paste(t, { "text/plain": "just words" })).toBe(false);
    expect(paste(t, { "text/plain": "# not me", "text/html": "<h1>not me</h1>" })).toBe(false);
  });

  it("does not convert inside a code block", async () => {
    const t = await make();
    load(t, [nodes.code_block.create()]);
    expect(paste(t, { "text/plain": "# comment" })).toBe(false);
  });

  it("turns pasted HTML into blocks through the schema", async () => {
    const t = await make();
    t.setText("");
    // The default paste path parses clipboard HTML with the schema's parse rules.
    const holder = document.createElement("div");
    holder.innerHTML = "<h2>Plan</h2><ul><li>one</li><li>two</li></ul><p>with <strong>bold</strong> and <a href='https://x.io'>a link</a></p>";
    const slice = ProseDomParser.fromSchema(pageSchema).parseSlice(holder);
    t.view.dispatch(t.view.state.tr.replaceSelection(slice));
    expect(t.topKinds().slice(0, 3)).toEqual(["heading", "bullet_list", "paragraph"]);
    const paragraph = t.view.state.doc.child(2);
    const names = new Set<string>();
    paragraph.forEach((child) => child.marks.forEach((m) => names.add(m.type.name)));
    expect([...names].sort()).toEqual(["bold", "link"]);
  });

  it("stores pasted images through the host and inserts an image block", async () => {
    const addImage = vi.fn().mockResolvedValue("attachment:abc");
    const t = await make({ addImage });
    t.setText("");
    const file = new File([new Uint8Array([1, 2, 3])], "shot.png", { type: "image/png" });
    expect(paste(t, {}, [file])).toBe(true);
    await settle();
    expect(addImage).toHaveBeenCalledWith(file);
    const image = t.view.state.doc.content.content.find((node) => node.type.name === "image") as PmNode;
    expect(image.attrs.src).toBe("attachment:abc");
    expect(image.attrs.alt).toBe("shot.png");
  });

  it("reports a failed image upload instead of dropping it silently", async () => {
    const onError = vi.fn();
    const t = await make({ addImage: () => Promise.reject(new Error("disk full")), onError });
    t.setText("");
    paste(t, {}, [new File([new Uint8Array([1])], "a.png", { type: "image/png" })]);
    await settle();
    expect(onError).toHaveBeenCalledWith("disk full");
  });
});

describe("block views", () => {
  it("opens the linked page on click and ignores trashed ones", async () => {
    const openPage = vi.fn();
    const state: { value: "ok" | "trashed" } = { value: "ok" };
    const t = await make({ openPage, resolvePage: () => ({ title: "Garden", state: state.value }) });
    t.setText("");
    insertChoice("page_link", { pageId: "p1", title: "old" })(t.view.state, t.view.dispatch);
    const card = t.view.dom.querySelector<HTMLElement>(".ne-page-card");
    expect(card?.textContent).toContain("Garden");
    card?.click();
    expect(openPage).toHaveBeenCalledWith("p1", false);

    state.value = "trashed";
    t.editor.refreshLinks();
    expect(card?.dataset.state).toBe("trashed");
    card?.click();
    expect(openPage).toHaveBeenCalledTimes(1);
  });

  it("shows child page blocks with their own hint and opens them", async () => {
    const openPage = vi.fn();
    const t = await make({ openPage, resolvePage: () => ({ title: "Sub", state: "ok" }) });
    t.setText("");
    insertChoice("child_page", { pageId: "c1", title: "Sub" })(t.view.state, t.view.dispatch);
    const card = t.view.dom.querySelector<HTMLElement>(".ne-child-page");
    expect(card?.textContent).toContain("Subpage");
    card?.dispatchEvent(new MouseEvent("click", { bubbles: true, ctrlKey: true }));
    expect(openPage).toHaveBeenCalledWith("c1", true);
  });

  it("opens an inline page link on click", async () => {
    const openPage = vi.fn();
    const t = await make({ openPage, resolvePage: () => ({ title: "T", state: "ok" }) });
    load(t, [nodes.paragraph.create(null, pageSchema.text("go", [marks.link.create({ href: "ne-page:p9" })]))]);
    const anchor = t.view.dom.querySelector<HTMLElement>("a.ne-inline-page");
    expect(anchor).not.toBeNull();
    const event = new MouseEvent("click", { bubbles: true, cancelable: true, ctrlKey: true });
    Object.defineProperty(event, "target", { value: anchor });
    const handled = t.view.someProp("handleClick", (f) => f(t.view, 2, event));
    expect(handled).toBe(true);
    expect(event.defaultPrevented).toBe(true);
    expect(openPage).toHaveBeenCalledWith("p9", true);
  });

  it("flips to-do items and toggles from their controls", async () => {
    const t = await make();
    load(t, [nodes.todo_list.create(null, nodes.todo_item.create(null, p("task"))), nodes.toggle.create(null, p("summary"))]);
    const box = t.view.dom.querySelector<HTMLInputElement>("li.ne-todo-item input");
    box?.click();
    expect((t.view.state.doc.firstChild?.firstChild as PmNode).attrs.checked).toBe(true);
    const button = t.view.dom.querySelector<HTMLButtonElement>(".ne-toggle-button");
    expect(button?.getAttribute("aria-expanded")).toBe("true");
    button?.click();
    expect(t.view.state.doc.child(1).attrs.open).toBe(false);
    expect(t.view.dom.querySelector<HTMLButtonElement>(".ne-toggle-button")?.getAttribute("aria-expanded")).toBe("false");
  });

  it("changes the language of a code block from its selector", async () => {
    const t = await make();
    load(t, [nodes.code_block.create({ language: "" }, pageSchema.text("x"))]);
    const select = t.view.dom.querySelector<HTMLSelectElement>("select.ne-code-language");
    expect(select).not.toBeNull();
    if (select) {
      select.value = "python";
      select.dispatchEvent(new Event("change", { bubbles: true }));
    }
    expect(t.view.state.doc.firstChild?.attrs.language).toBe("python");
  });

  it("resolves attachment references for images", async () => {
    const resolveImage = vi.fn().mockResolvedValue("blob:resolved");
    const t = await make({ resolveImage });
    load(t, [nodes.image.create({ src: "attachment:abc", alt: "pic" })]);
    await settle();
    expect(resolveImage).toHaveBeenCalledWith("attachment:abc");
    expect(t.view.dom.querySelector("img")?.getAttribute("src")).toBe("blob:resolved");
  });
});
