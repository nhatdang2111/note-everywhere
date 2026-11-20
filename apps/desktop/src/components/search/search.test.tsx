import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import type { LoroDoc } from "loro-crdt";
import type { SearchHit } from "../../api/contract.ts";
import { appendParagraph } from "../../api/page-doc.ts";
import { renderApp, seedLocal, tick, type RenderedApp } from "../../test/app-harness.tsx";

async function openWithPages() {
  const ui = await renderApp({
    setup: async (memory) => {
      await seedLocal(memory);
      const folder = await memory.api.treeCreate({ kind: "folder", parentId: null, title: "Công việc" });
      const meeting = await memory.api.treeCreate({ kind: "page", parentId: folder, title: "Họp tuần" });
      appendParagraph(memory.control.pageDoc(meeting) as LoroDoc, "Nội dung ghi chú quan trọng của buổi họp");
      const quick = await memory.api.treeCreate({ kind: "page", parentId: null, title: "Ghi chú nhanh" });
      appendParagraph(memory.control.pageDoc(quick) as LoroDoc, "không liên quan");
      const other = await memory.api.treeCreate({ kind: "page", parentId: null, title: "Garden" });
      appendParagraph(memory.control.pageDoc(other) as LoroDoc, "tomatoes and basil");
    },
  });
  await screen.findByRole("navigation", { name: "Workspace" });
  return ui;
}

const openPalette = async (_ui?: RenderedApp) => {
  fireEvent.keyDown(window, { key: "k", ctrlKey: true });
  const dialog = await screen.findByRole("dialog", { name: "Search pages" });
  return { dialog, input: within(dialog).getByRole("combobox") as HTMLInputElement };
};

const options = (dialog: HTMLElement) => within(dialog).queryAllByRole("option");

describe("quick search", () => {
  it("opens with Ctrl+K, focuses the box and closes with Escape or the same shortcut", async () => {
    const ui = await openWithPages();
    const { input } = await openPalette(ui);
    await waitFor(() => expect(document.activeElement).toBe(input));
    await ui.user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Search pages" })).toBeNull());
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await screen.findByRole("dialog", { name: "Search pages" });
    fireEvent.keyDown(window, { key: "k", ctrlKey: true });
    await waitFor(() => expect(screen.queryByRole("dialog", { name: "Search pages" })).toBeNull());
  });

  it("shows recent pages when the box is empty and opens one with the keyboard", async () => {
    const ui = await openWithPages();
    const pages = await ui.memory.api.treeSnapshot();
    ui.app.openPage(pages.find((p) => p.title === "Garden")?.id as string);
    ui.app.openPage(pages.find((p) => p.title === "Ghi chú nhanh")?.id as string);
    const { dialog, input } = await openPalette(ui);
    expect(within(dialog).getByText("Recent pages")).toBeTruthy();
    expect(options(dialog).map((o) => o.querySelector(".palette-title")?.textContent)).toEqual(["Ghi chú nhanh", "Garden"]);
    await ui.user.click(input);
    await ui.user.keyboard("{ArrowDown}{Enter}");
    await waitFor(() => expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Garden"));
    expect(screen.queryByRole("dialog", { name: "Search pages" })).toBeNull();
  });

  it("says so when there is nothing recent", async () => {
    const ui = await openWithPages();
    const { dialog } = await openPalette(ui);
    expect(within(dialog).getByText("Pages you open will show up here.")).toBeTruthy();
  });

  it("finds pages ignoring accents, with breadcrumb and highlighted snippet", async () => {
    const ui = await openWithPages();
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "ghi chu");
    await waitFor(() => expect(options(dialog).length).toBe(3));
    const titles = options(dialog).map((o) => o.querySelector(".palette-title")?.textContent);
    expect(titles.slice(0, 2)).toEqual(["Ghi chú nhanh", "Họp tuần"]);
    expect(titles[2]).toBe("Create page “ghi chu”");

    const meeting = options(dialog)[1] as HTMLElement;
    expect(meeting.querySelector(".palette-crumb")?.textContent).toBe("Công việc");
    const marks = [...meeting.querySelectorAll("mark")].map((m) => m.textContent?.toLowerCase());
    expect(marks.length).toBeGreaterThan(0);
    expect(marks.join(" ")).toMatch(/ghi|chú/);
  });

  it("moves through results with the arrow keys and opens the selected one", async () => {
    const ui = await openWithPages();
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "ghi chu");
    await waitFor(() => expect(options(dialog).length).toBe(3));
    expect(options(dialog)[0]?.getAttribute("aria-selected")).toBe("true");
    expect(input.getAttribute("aria-activedescendant")).toBe(options(dialog)[0]?.id);
    await ui.user.keyboard("{ArrowDown}");
    expect(options(dialog)[1]?.getAttribute("aria-selected")).toBe("true");
    await ui.user.keyboard("{ArrowUp}{ArrowUp}");
    expect(options(dialog)[2]?.getAttribute("aria-selected")).toBe("true");
    await ui.user.keyboard("{ArrowDown}{ArrowDown}{Enter}");
    await waitFor(() => expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Họp tuần"));
  });

  it("opens in a new tab with Ctrl+Enter", async () => {
    const ui = await openWithPages();
    ui.app.openPage((await ui.memory.api.treeSnapshot()).find((p) => p.title === "Garden")?.id as string);
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "ghi chu");
    await waitFor(() => expect(options(dialog).length).toBe(3));
    await ui.user.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(screen.getAllByRole("tab")).toHaveLength(2));
  });

  it("creates a page from the query", async () => {
    const ui = await openWithPages();
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "Brand new idea");
    await waitFor(() => expect(options(dialog).at(-1)?.textContent).toContain("Create page “Brand new idea”"));
    await ui.user.click(options(dialog).at(-1) as HTMLElement);
    await waitFor(async () => expect((await ui.memory.api.treeSnapshot()).some((n) => n.title === "Brand new idea")).toBe(true));
    await waitFor(() => expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Brand new idea"));
  });

  it("offers to create a page when nothing matches", async () => {
    const ui = await openWithPages();
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "zzzz");
    expect(await within(dialog).findByText(/Nothing found/)).toBeTruthy();
    expect(options(dialog)).toHaveLength(1);
  });

  it("ignores a slow answer to an older query", async () => {
    const ui = await openWithPages();
    const real = ui.memory.api.searchQuery;
    const answers: Record<string, { delay: number; hits: SearchHit[] }> = {
      g: { delay: 250, hits: [{ pageId: "x", title: "Stale result", breadcrumb: [], snippet: "old", highlights: [] }] },
    };
    ui.memory.api.searchQuery = async (args) => {
      const planned = answers[args.text];
      if (planned) {
        await tick(planned.delay);
        return planned.hits;
      }
      return real(args);
    };
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "g");
    await tick(200);
    await ui.user.type(input, "arden");
    await waitFor(() => expect(options(dialog)[0]?.querySelector(".palette-title")?.textContent).toBe("Garden"));
    await tick(300);
    expect(within(dialog).queryByText("Stale result")).toBeNull();
  });

  it("shows the error when searching fails", async () => {
    const ui = await openWithPages();
    ui.memory.api.searchQuery = () => Promise.reject({ code: "index_busy", message: "The search index is rebuilding." });
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "abc");
    expect((await within(dialog).findByRole("alert")).textContent).toContain("The search index is rebuilding.");
  });

  it("clears results when the box is emptied again", async () => {
    const ui = await openWithPages();
    const { dialog, input } = await openPalette(ui);
    await ui.user.type(input, "garden");
    await waitFor(() => expect(options(dialog).length).toBeGreaterThan(0));
    await ui.user.clear(input);
    await waitFor(() => expect(within(dialog).queryByText(/Create page/)).toBeNull());
  });
});
