import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import { TextSelection } from "prosemirror-state";
import type { PageEditor } from "../../editor/index.ts";
import { pressKey, setParagraph, typeInto } from "../../test/editor-harness.ts";
import { renderApp, seedLocal, tick, type RenderedApp } from "../../test/app-harness.tsx";

async function openPage(options: Parameters<typeof renderApp>[0] = {}, titles = ["Alpha", "Beta", "Gamma"]) {
  const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory, titles)), ...options });
  const [first] = await ui.memory.api.treeSnapshot();
  await screen.findByRole("navigation", { name: "Workspace" });
  ui.app.openPage(first?.id as string);
  const editor = await waitFor(() => {
    const current = ui.app.ui.activeEditor();
    if (!current) throw new Error("editor not ready");
    return current;
  });
  return { ui, editor, pageId: first?.id as string, view: editor.view };
}

const focusEditor = (editor: PageEditor) => {
  editor.view.dom.setAttribute("tabindex", "0");
  editor.view.focus();
};

const storedText = (ui: RenderedApp, pageId: string) => JSON.stringify(ui.memory.control.pageDoc(pageId)?.getMap("doc").toJSON());

describe("page view", () => {
  it("shows the title from the tree and renames the node when the title changes", async () => {
    const { ui, pageId } = await openPage();
    const title = screen.getByLabelText("Page title") as HTMLInputElement;
    expect(title.value).toBe("Alpha");
    await ui.user.clear(title);
    await ui.user.type(title, "Renamed page");
    await ui.user.tab();
    await waitFor(async () => expect((await ui.memory.api.treeSnapshot()).find((n) => n.id === pageId)?.title).toBe("Renamed page"));
    expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Renamed page");
  });

  it("moves from the title into the text with Enter", async () => {
    const { ui, view } = await openPage();
    const title = screen.getByLabelText("Page title");
    title.focus();
    await ui.user.keyboard("{Enter}");
    expect(view.state.selection.from).toBe(1);
  });

  it("sends local edits to the core and shows them again after reopening", async () => {
    const { ui, editor, pageId, view } = await openPage();
    setParagraph(view, "");
    typeInto(view, "hello from the editor");
    await waitFor(() => expect(storedText(ui, pageId)).toContain("hello from the editor"));
    expect(editor.view.state.doc.textContent).toBe("hello from the editor");

    // Close the page (closing the tab shows Home), then open it again from the sidebar.
    await ui.user.click(screen.getByRole("button", { name: /^Close/ }));
    await waitFor(() => expect(ui.app.ui.activeEditor()).toBeNull());
    await ui.user.click(within(screen.getByRole("tree")).getByText("Alpha"));
    const again = await waitFor(() => {
      const current = ui.app.ui.activeEditor();
      if (!current) throw new Error("editor not ready");
      return current;
    });
    expect(again.view.state.doc.textContent).toBe("hello from the editor");
  });

  it("applies a remote update to the open editor and keeps the caret on its words", async () => {
    const { ui, editor, pageId, view } = await openPage();
    setParagraph(view, "Hello world", 5);
    await waitFor(() => expect(storedText(ui, pageId)).toContain("Hello world"));
    // Let the binding finish its own deferred work before placing the caret.
    await tick(40);
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 6)));
    const peer = new LoroDoc();
    peer.import((await ui.memory.api.pageOpen({ id: pageId })).snapshot);
    const before = peer.version();
    const paragraph = (peer.getMap("doc").get("children") as LoroList).get(0) as LoroMap;
    const run = (paragraph.get("children") as LoroList).get(0) as LoroText;
    run.insert(0, "Dear ");
    peer.commit();
    ui.memory.control.receiveRemoteUpdate(pageId, peer.export({ mode: "update", from: before }));
    await waitFor(() => expect(editor.view.state.doc.textContent).toBe("Dear Hello world"));
    await tick(50);
    expect(editor.view.state.selection.from).toBe(1 + "Dear Hello".length);
  });

  it("explains a failed save and retries it", async () => {
    const { ui, view, pageId } = await openPage();
    const original = ui.memory.api.pageApplyUpdate;
    let fail = true;
    ui.memory.api.pageApplyUpdate = (args) => (fail ? Promise.reject({ code: "disk_full", message: "No space left." }) : original(args));
    setParagraph(view, "");
    typeInto(view, "unsaved");
    const banner = await screen.findByText(/could not be saved: No space left\./);
    expect(banner).toBeTruthy();
    fail = false;
    await ui.user.click(screen.getByRole("button", { name: "Try again" }));
    await waitFor(() => expect(screen.queryByText(/could not be saved/)).toBeNull());
    await waitFor(() => expect(storedText(ui, pageId)).toContain("unsaved"));
  });

  it("shows why a page cannot be opened", async () => {
    const ui = await renderApp({
      setup: async (memory) => {
        await seedLocal(memory, ["Alpha"]);
        const original = memory.api.pageOpen;
        memory.api.pageOpen = () => Promise.reject({ code: "corrupt", message: "This page is damaged." });
        void original;
      },
    });
    const [page] = await ui.memory.api.treeSnapshot();
    ui.app.openPage(page?.id as string);
    expect((await screen.findByText("This page is damaged.")).closest('[role="alert"]')).not.toBeNull();
  });
});

describe("slash menu", () => {
  const openMenu = async (text = "/") => {
    const page = await openPage();
    setParagraph(page.view, "");
    typeInto(page.view, text);
    const list = await screen.findByRole("listbox", { name: "Insert a block" });
    return { ...page, list };
  };

  it("lists all 15 block kinds, a database and an existing synced block when opened", async () => {
    const { list } = await openMenu();
    expect(within(list).getAllByRole("option")).toHaveLength(17);
  });

  it("filters while typing and shows nothing-found text", async () => {
    const { list, view } = await openMenu("/head");
    expect(within(list).getAllByRole("option").map((o) => o.textContent)).toEqual([
      expect.stringContaining("Heading 1"),
      expect.stringContaining("Heading 2"),
      expect.stringContaining("Heading 3"),
    ]);
    typeInto(view, "zzz");
    await waitFor(() => expect(within(screen.getByRole("listbox", { name: "Insert a block" })).queryAllByRole("option")).toHaveLength(0));
    expect(screen.getByText("No block matches.")).toBeTruthy();
  });

  it("moves with the arrow keys and inserts with Enter", async () => {
    const { view } = await openMenu("/head");
    const press = (key: string) => fireEvent.keyDown(view.dom, { key });
    // The editor owns the keyboard; drive it the way ProseMirror receives it.
    expect(pressKey(view, "ArrowDown")).toBe(true);
    await waitFor(() => expect(screen.getAllByRole("option")[1]?.getAttribute("aria-selected")).toBe("true"));
    expect(view.dom.getAttribute("aria-activedescendant")).toBe(screen.getAllByRole("option")[1]?.id);
    expect(pressKey(view, "Enter")).toBe(true);
    void press;
    await waitFor(() => expect(view.state.doc.firstChild?.type.name).toBe("heading"));
    expect(view.state.doc.firstChild?.attrs.level).toBe(2);
    expect(view.state.doc.textContent).toBe("");
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
  });

  it("wraps around at the ends", async () => {
    const { view } = await openMenu("/head");
    pressKey(view, "ArrowUp");
    await waitFor(() => expect(screen.getAllByRole("option")[2]?.getAttribute("aria-selected")).toBe("true"));
    pressKey(view, "ArrowDown");
    await waitFor(() => expect(screen.getAllByRole("option")[0]?.getAttribute("aria-selected")).toBe("true"));
  });

  it("closes on Escape and leaves the typed text alone", async () => {
    const { view } = await openMenu("/head");
    expect(pressKey(view, "Escape")).toBe(true);
    await waitFor(() => expect(screen.queryByRole("listbox")).toBeNull());
    expect(view.state.doc.textContent).toBe("/head");
  });

  it("inserts the clicked kind", async () => {
    const { list, view } = await openMenu("/");
    await userClick(within(list).getByText("To-do list"));
    await waitFor(() => expect(view.state.doc.firstChild?.type.name).toBe("todo_list"));
  });

  it("creates a subpage and links it from a child page block", async () => {
    const { ui, list, view, pageId } = await openMenu("/sub");
    await userClick(within(list).getByText("Subpage"));
    await waitFor(() => expect(view.state.doc.firstChild?.type.name).toBe("child_page"));
    const childId = view.state.doc.firstChild?.attrs.pageId as string;
    expect(childId).not.toBe("");
    const snapshot = await ui.memory.api.treeSnapshot();
    expect(snapshot.find((node) => node.id === childId)?.parentId).toBe(pageId);
  });

  it("inserts a link-to-page block chosen in the page dialog", async () => {
    const { list, view } = await openMenu("/link");
    await userClick(within(list).getByText("Link to page"));
    const dialog = await screen.findByRole("dialog", { name: "Choose a page" });
    const options = within(dialog).getAllByRole("option");
    expect(options.map((o) => o.textContent).join(" ")).not.toContain("Alpha");
    await userClick(within(dialog).getByText("Gamma"));
    await waitFor(() => expect(view.state.doc.firstChild?.type.name).toBe("page_link"));
    expect(view.state.doc.firstChild?.attrs.pageId).not.toBe("");
    await waitFor(() => expect(view.dom.querySelector(".ne-page-link")?.textContent).toContain("Gamma"));
  });

  it("inserts an image picked through the attachment API", async () => {
    const file = new File([new Uint8Array([137, 80, 78, 71])], "pic.png", { type: "image/png" });
    const page = await openPage({ pickFiles: async () => [file] });
    setParagraph(page.view, "");
    typeInto(page.view, "/image");
    const list = await screen.findByRole("listbox", { name: "Insert a block" });
    await userClick(within(list).getByText("Image"));
    await waitFor(() => expect(page.view.state.doc.firstChild?.type.name).toBe("image"));
    expect(String(page.view.state.doc.firstChild?.attrs.src)).toMatch(/^attachment:/);
    await waitFor(() => expect(page.view.dom.querySelector("img")?.getAttribute("src")).toBeTruthy());
  });

  async function userClick(element: HTMLElement) {
    fireEvent.mouseDown(element);
    fireEvent.click(element);
  }
});

describe("page link picker", () => {
  const openPicker = async (text: string) => {
    const page = await openPage();
    setParagraph(page.view, "see ");
    typeInto(page.view, text);
    return { ...page, list: await screen.findByRole("listbox", { name: "Link to a page" }) };
  };

  it("lists other pages for [[ and never the page itself", async () => {
    const { list } = await openPicker("[[");
    const names = within(list).getAllByRole("option").map((o) => o.textContent);
    expect(names.some((name) => name?.includes("Beta"))).toBe(true);
    expect(names.some((name) => name?.includes("Alpha"))).toBe(false);
  });

  it("inserts an inline link with Enter", async () => {
    const { view, ui } = await openPicker("[[gam");
    expect(within(screen.getByRole("listbox", { name: "Link to a page" })).getAllByRole("option")[0]?.textContent).toContain("Gamma");
    expect(pressKey(view, "Enter")).toBe(true);
    const gamma = (await ui.memory.api.treeSnapshot()).find((node) => node.title === "Gamma");
    await waitFor(() => expect(view.state.doc.textContent).toBe("see Gamma "));
    expect(view.state.doc.firstChild?.child(1).marks[0]?.attrs.href).toBe(`ne-page:${gamma?.id}`);
  });

  it("works with @ and filters by accent-free text", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory, ["Alpha", "Ghi chú họp"])) });
    const [first] = await ui.memory.api.treeSnapshot();
    ui.app.openPage(first?.id as string);
    const editor = await waitFor(() => ui.app.ui.activeEditor() ?? Promise.reject(new Error("no editor")));
    setParagraph(editor.view, "ping ");
    typeInto(editor.view, "@ghi");
    const list = await screen.findByRole("listbox", { name: "Link to a page" });
    expect(within(list).getAllByRole("option")[0]?.textContent).toContain("Ghi chú họp");
  });

  it("offers to create a page from what was typed", async () => {
    const { view, ui } = await openPicker("[[Brand new idea");
    const create = within(screen.getByRole("listbox", { name: "Link to a page" })).getByText("Create “Brand new idea”");
    fireEvent.mouseDown(create);
    fireEvent.click(create);
    await waitFor(async () => expect((await ui.memory.api.treeSnapshot()).some((node) => node.title === "Brand new idea")).toBe(true));
    await waitFor(() => expect(view.state.doc.textContent).toBe("see Brand new idea "));
  });

  it("opens the linked page when the inline link is clicked", async () => {
    const { view, ui } = await openPicker("[[bet");
    pressKey(view, "Enter");
    await waitFor(() => expect(view.dom.querySelector("a.ne-inline-page")).not.toBeNull());
    const anchor = view.dom.querySelector("a.ne-inline-page") as HTMLElement;
    const event = new MouseEvent("click", { bubbles: true, cancelable: true });
    Object.defineProperty(event, "target", { value: anchor });
    view.someProp("handleClick", (f) => f(view, 6, event));
    await waitFor(() => expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Beta"));
    expect(ui.app.tabs.activePageId()).not.toBeNull();
  });
});

describe("formatting toolbar and links", () => {
  const select = async (text: string, from: number, to: number) => {
    const page = await openPage();
    setParagraph(page.view, text);
    focusEditor(page.editor);
    page.view.dispatch(page.view.state.tr.setSelection(TextSelection.create(page.view.state.doc, from, to)));
    return page;
  };

  it("appears over selected text and toggles bold and italic", async () => {
    const { view } = await select("make this bold", 6, 10);
    const toolbar = await screen.findByRole("toolbar", { name: "Text formatting" });
    const bold = within(toolbar).getByRole("button", { name: /^Bold/ });
    fireEvent.click(bold);
    expect(view.state.doc.firstChild?.child(1).marks[0]?.type.name).toBe("bold");
    await waitFor(() => expect(within(screen.getByRole("toolbar")).getByRole("button", { name: /^Bold/ }).getAttribute("aria-pressed")).toBe("true"));
    fireEvent.click(within(screen.getByRole("toolbar")).getByRole("button", { name: /^Italic/ }));
    expect(view.state.doc.firstChild?.child(1).marks.map((m) => m.type.name).sort()).toEqual(["bold", "italic"]);
    fireEvent.click(within(screen.getByRole("toolbar")).getByRole("button", { name: /^Bold/ }));
    expect(view.state.doc.firstChild?.child(1).marks.map((m) => m.type.name)).toEqual(["italic"]);
  });

  it("is hidden when nothing is selected", async () => {
    const { view } = await select("plain", 3, 3);
    expect(view.state.selection.empty).toBe(true);
    expect(screen.queryByRole("toolbar")).toBeNull();
  });

  it("adds a link, adding https:// to a bare domain", async () => {
    const { view } = await select("visit docs", 7, 11);
    const toolbar = await screen.findByRole("toolbar");
    fireEvent.click(within(toolbar).getByRole("button", { name: /^Link/ }));
    const input = (await screen.findByLabelText("Link address")) as HTMLInputElement;
    fireEvent.input(input, { target: { value: "example.com/docs" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);
    await waitFor(() => expect(view.state.doc.firstChild?.child(1).marks[0]?.attrs.href).toBe("https://example.com/docs"));
  });

  it("refuses an address that could run code", async () => {
    const { view } = await select("visit docs", 7, 11);
    fireEvent.click(within(await screen.findByRole("toolbar")).getByRole("button", { name: /^Link/ }));
    const input = (await screen.findByLabelText("Link address")) as HTMLInputElement;
    fireEvent.input(input, { target: { value: "javascript:alert(1)" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);
    expect((await screen.findByRole("alert")).textContent).toContain("web, email or phone");
    expect(view.state.doc.firstChild?.childCount).toBe(1);
  });

  it("edits and removes a link from the bar that shows when the cursor is inside it", async () => {
    const page = await openPage();
    const { view } = page;
    setParagraph(view, "x");
    const { schema } = view.state;
    view.dispatch(view.state.tr.replaceWith(1, 2, schema.text("docs", [schema.marks.link.create({ href: "https://old.example" })])));
    focusEditor(page.editor);
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 3)));
    const bar = await screen.findByRole("toolbar");
    expect(within(bar).getByText("https://old.example")).toBeTruthy();
    fireEvent.click(within(bar).getByRole("button", { name: "Edit" }));
    const input = (await screen.findByLabelText("Link address")) as HTMLInputElement;
    expect(input.value).toBe("https://old.example");
    fireEvent.input(input, { target: { value: "https://new.example" } });
    fireEvent.submit(input.closest("form") as HTMLFormElement);
    await waitFor(() => expect(view.state.doc.firstChild?.firstChild?.marks[0]?.attrs.href).toBe("https://new.example"));
    fireEvent.click(within(await screen.findByRole("toolbar")).getByRole("button", { name: "Remove link" }));
    await waitFor(() => expect(view.state.doc.firstChild?.firstChild?.marks).toHaveLength(0));
  });

  it("opens the link editor from the keyboard shortcut only when it can act", async () => {
    const { view, editor } = await select("pick me", 1, 5);
    expect(pressKey(view, "k", { ctrlKey: true, shiftKey: true })).toBe(true);
    expect(await screen.findByLabelText("Link address")).toBeTruthy();
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 3)));
    editor.view.focus();
    fireEvent.keyDown(screen.getByLabelText("Link address"), { key: "Escape" });
    await waitFor(() => expect(screen.queryByLabelText("Link address")).toBeNull());
    expect(pressKey(view, "k", { ctrlKey: true, shiftKey: true })).toBe(false);
  });
});

describe("find in page", () => {
  it("opens with Ctrl+F, counts matches, steps through them and closes with Escape", async () => {
    const { ui, view } = await openPage();
    setParagraph(view, "Ghi chú một. ghi chu hai. Ghi chú ba.");
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const box = (await screen.findByRole("search", { name: "Find in page" })).querySelector("input") as HTMLInputElement;
    await ui.user.type(box, "ghi chu");
    expect(await screen.findByText("1 of 3")).toBeTruthy();
    await ui.user.keyboard("{Enter}");
    expect(await screen.findByText("2 of 3")).toBeTruthy();
    await ui.user.keyboard("{Shift>}{Enter}{/Shift}");
    expect(await screen.findByText("1 of 3")).toBeTruthy();
    expect(view.dom.querySelectorAll(".ne-find-match")).toHaveLength(3);
    await ui.user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("search", { name: "Find in page" })).toBeNull());
    expect(view.dom.querySelectorAll(".ne-find-match")).toHaveLength(0);
  });

  it("says when there is no match", async () => {
    const { ui, view } = await openPage();
    setParagraph(view, "nothing");
    fireEvent.keyDown(window, { key: "f", ctrlKey: true });
    const box = (await screen.findByRole("search", { name: "Find in page" })).querySelector("input") as HTMLInputElement;
    await ui.user.type(box, "zebra");
    expect(await screen.findByText("No matches")).toBeTruthy();
  });
});

describe("block menu", () => {
  const withBlocks = async () => {
    const page = await openPage();
    const { schema } = page.view.state;
    const paragraph = (text: string) => schema.node("paragraph", null, schema.text(text));
    page.view.dispatch(page.view.state.tr.replaceWith(0, page.view.state.doc.content.size, [paragraph("first"), paragraph("second")]));
    await tick(40);
    page.view.dispatch(page.view.state.tr.setSelection(TextSelection.create(page.view.state.doc, 2)));
    return page;
  };

  it("opens from the keyboard and turns the block into another kind", async () => {
    const { view, ui } = await withBlocks();
    expect(pressKey(view, "/", { ctrlKey: true })).toBe(true);
    const menu = await screen.findByRole("menu", { name: "Block actions" });
    await ui.user.click(within(menu).getByRole("menuitem", { name: "Heading 1" }));
    expect(view.state.doc.firstChild?.type.name).toBe("heading");
    expect(view.state.doc.firstChild?.textContent).toBe("first");
  });

  it("duplicates, moves and deletes", async () => {
    const { view, ui } = await withBlocks();
    const run = async (name: string) => {
      pressKey(view, "/", { ctrlKey: true });
      await ui.user.click(within(await screen.findByRole("menu", { name: "Block actions" })).getByRole("menuitem", { name }));
    };
    const texts = () => {
      const out: string[] = [];
      view.state.doc.forEach((node) => out.push(node.textContent));
      return out;
    };
    await run("Duplicate");
    expect(texts()).toEqual(["first", "first", "second"]);
    await run("Move down");
    expect(texts()).toEqual(["first", "second", "first"]);
    await run("Delete");
    expect(texts()).toEqual(["first", "second"]);
  });

  it("selects the current block with Escape and extends with Shift+Arrow", async () => {
    const { view } = await withBlocks();
    expect(pressKey(view, "Escape")).toBe(true);
    expect(view.state.selection.constructor.name).not.toBe("TextSelection");
    pressKey(view, "ArrowDown", { shiftKey: true });
    await waitFor(() => expect(view.dom.querySelectorAll(".ne-block-selected")).toHaveLength(2));
  });

  it("closes with Escape", async () => {
    const { view, ui } = await withBlocks();
    pressKey(view, "/", { ctrlKey: true });
    await screen.findByRole("menu", { name: "Block actions" });
    await ui.user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("menu")).toBeNull());
  });
});

describe("navigation from blocks and backlinks", () => {
  it("lists pages that link here and opens one", async () => {
    const page = await openPage();
    const [alpha, beta] = await page.ui.memory.api.treeSnapshot();
    const doc = page.ui.memory.control.pageDoc(beta?.id as string) as LoroDoc;
    const list = doc.getMap("doc").get("children") as LoroList;
    const node = list.insertContainer(list.length, new LoroMap());
    node.set("nodeName", "page_link");
    const attrs = node.setContainer("attributes", new LoroMap());
    attrs.set("pageId", alpha?.id as string);
    attrs.set("title", "Alpha");
    node.setContainer("children", new LoroList());
    doc.commit();
    // Any tree change prompts the panel to look again.
    await page.ui.memory.api.treeRename({ id: beta?.id as string, title: "Beta" });
    const panel = await screen.findByRole("heading", { name: /Linked from/ });
    const section = panel.closest("section") as HTMLElement;
    await waitFor(() => expect(within(section).getByText("Beta")).toBeTruthy());
    fireEvent.click(within(section).getByText("Beta"));
    await waitFor(() => expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Beta"));
  });

  it("says when nothing links here", async () => {
    await openPage();
    expect(await screen.findByText("No other page links here yet.")).toBeTruthy();
  });

  it("opens the page behind a link block when it is clicked, and refreshes its title", async () => {
    const { ui, view } = await openPage();
    const gamma = (await ui.memory.api.treeSnapshot()).find((node) => node.title === "Gamma");
    const { schema } = view.state;
    view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, [schema.node("page_link", { pageId: gamma?.id as string, title: "stale" })]));
    const card = await waitFor(() => view.dom.querySelector<HTMLElement>(".ne-page-link") ?? Promise.reject(new Error("no card")));
    expect(card.textContent).toContain("Gamma");
    await ui.memory.api.treeRename({ id: gamma?.id as string, title: "Gamma renamed" });
    await waitFor(() => expect(card.textContent).toContain("Gamma renamed"));
    fireEvent.click(card);
    await waitFor(() => expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Gamma renamed"));
  });

  it("dims a link to a trashed page and does not open it", async () => {
    const { ui, view } = await openPage();
    const gamma = (await ui.memory.api.treeSnapshot()).find((node) => node.title === "Gamma");
    const { schema } = view.state;
    view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, [schema.node("page_link", { pageId: gamma?.id as string, title: "Gamma" })]));
    await ui.app.tree.refreshTrash();
    await ui.memory.api.treeTrash({ id: gamma?.id as string });
    await ui.app.tree.refreshTrash();
    const card = await waitFor(() => view.dom.querySelector<HTMLElement>(".ne-page-link[data-state='trashed']") ?? Promise.reject(new Error("not dimmed")));
    expect(card.textContent).toContain("In the trash");
    fireEvent.click(card);
    expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Alpha");
    // Restoring brings the link back to life.
    await ui.memory.api.treeRestore({ id: gamma?.id as string });
    await waitFor(() => expect(card.dataset.state).toBe("ok"));
  });
});
