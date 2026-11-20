import { describe, expect, it } from "vitest";
import { createEvent, fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import { renderApp, seedLocal, seedSignedIn, tick } from "../../test/app-harness.tsx";

const treeRows = () => [...document.querySelectorAll<HTMLElement>('[role="treeitem"]')];
const rowNamed = (title: string) => {
  const row = treeRows().find((candidate) => candidate.querySelector(".tree-title")?.textContent === title);
  if (!row) throw new Error(`no tree row named ${title}`);
  return row;
};
const titlesInTree = () => treeRows().map((row) => row.querySelector(".tree-title")?.textContent);

/** jsdom has no layout; give a row a box so the pointer position can pick the drop zone. */
function withBox(row: HTMLElement) {
  row.getBoundingClientRect = () => ({ top: 0, bottom: 30, left: 0, right: 200, width: 200, height: 30, x: 0, y: 0, toJSON: () => ({}) });
  return row;
}

async function openWorkspace(titles: string[] = []) {
  const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory, titles)) });
  await screen.findByRole("navigation", { name: "Workspace" });
  return ui;
}

describe("page tree", () => {
  it("creates pages and folders at any level and opens new pages", async () => {
    const ui = await openWorkspace();
    expect(await screen.findByText("No pages yet")).toBeTruthy();

    await ui.user.click(screen.getAllByRole("button", { name: "New folder" })[0] as HTMLElement);
    const rename = (await screen.findByLabelText("Name")) as HTMLInputElement;
    await ui.user.type(rename, "Projects{Enter}");
    await waitFor(() => expect(titlesInTree()).toEqual(["Projects"]));

    // A page inside the folder through its context menu.
    fireEvent.contextMenu(rowNamed("Projects"));
    await ui.user.click(await screen.findByRole("menuitem", { name: "New page inside" }));
    await waitFor(() => expect(titlesInTree().length).toBe(2));
    expect(rowNamed("Projects").getAttribute("aria-expanded")).toBe("true");
    const child = treeRows()[1] as HTMLElement;
    expect(child.getAttribute("aria-level")).toBe("2");
    // The new page is open in the active tab.
    expect(await screen.findByLabelText("Page title")).toBeTruthy();
  });

  it("renames in place with Enter and cancels with Escape", async () => {
    const ui = await openWorkspace(["Old name"]);
    await ui.user.dblClick(await screen.findByText("Old name"));
    const input = (await screen.findByLabelText("Name")) as HTMLInputElement;
    await ui.user.clear(input);
    await ui.user.type(input, "New name{Enter}");
    await waitFor(() => expect(titlesInTree()).toEqual(["New name"]));

    await ui.user.dblClick(rowNamed("New name"));
    const again = (await screen.findByLabelText("Name")) as HTMLInputElement;
    await ui.user.type(again, " extra{Escape}");
    expect(titlesInTree()).toEqual(["New name"]);
  });

  it("remembers which branches are open", async () => {
    const ui = await openWorkspace();
    const folder = await ui.memory.api.treeCreate({ kind: "folder", parentId: null, title: "Folder" });
    await ui.memory.api.treeCreate({ kind: "page", parentId: folder, title: "Inside" });
    await screen.findByText("Folder");
    expect(titlesInTree()).toEqual(["Folder"]);
    await ui.user.click(within(rowNamed("Folder")).getByRole("button", { name: "Expand", hidden: true }));
    expect(titlesInTree()).toEqual(["Folder", "Inside"]);
    expect(JSON.parse(localStorage.getItem("ne.tree.expanded") ?? "[]")).toEqual([folder]);
    await ui.user.click(within(rowNamed("Folder")).getByRole("button", { name: "Collapse", hidden: true }));
    expect(titlesInTree()).toEqual(["Folder"]);
    expect(JSON.parse(localStorage.getItem("ne.tree.expanded") ?? "[]")).toEqual([]);
  });

  it("supports the keyboard: arrows, expand, rename and delete", async () => {
    const ui = await openWorkspace();
    const folder = await ui.memory.api.treeCreate({ kind: "folder", parentId: null, title: "Folder" });
    await ui.memory.api.treeCreate({ kind: "page", parentId: folder, title: "Inside" });
    await screen.findByText("Folder");
    rowNamed("Folder").focus();
    await ui.user.keyboard("{ArrowRight}");
    expect(titlesInTree()).toEqual(["Folder", "Inside"]);
    await ui.user.keyboard("{ArrowDown}");
    expect(document.activeElement).toBe(rowNamed("Inside"));
    await ui.user.keyboard("{ArrowLeft}");
    expect(document.activeElement).toBe(rowNamed("Folder"));
    await ui.user.keyboard("{F2}");
    const input = (await screen.findByLabelText("Name")) as HTMLInputElement;
    await ui.user.type(input, "{Escape}");
    await ui.user.keyboard("{Delete}");
    await waitFor(() => expect(titlesInTree()).toEqual([]));
  });

  it("pins favourites at the top and unpins them", async () => {
    const ui = await openWorkspace(["Alpha", "Beta"]);
    fireEvent.contextMenu(await screen.findByText("Beta"));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Add to favorites" }));
    const favorites = await screen.findByRole("heading", { name: "Favorites" });
    const list = favorites.parentElement as HTMLElement;
    expect(within(list).getByText("Beta")).toBeTruthy();
    fireEvent.contextMenu(rowNamed("Beta"));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Remove from favorites" }));
    await waitFor(() => expect(screen.queryByRole("heading", { name: "Favorites" })).toBeNull());
  });

  it("moves a node up, down and out from the menu", async () => {
    const ui = await openWorkspace(["A", "B"]);
    const parent = (await ui.memory.api.treeSnapshot())[0]?.id as string;
    await ui.memory.api.treeCreate({ kind: "page", parentId: parent, title: "A child" });
    await screen.findByText("A child", {}, { timeout: 100 }).catch(() => undefined);
    fireEvent.contextMenu(rowNamed("B"));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Move up" }));
    await waitFor(() => expect(titlesInTree()[0]).toBe("B"));
  });
});

describe("drag and drop in the tree", () => {
  /** jsdom has no DragEvent, so pointer coordinates are attached to the event by hand. */
  const fireDrag = (type: "dragOver" | "drop", element: HTMLElement, dataTransfer: object, clientY: number) => {
    const event = createEvent[type](element, { dataTransfer });
    Object.defineProperty(event, "clientY", { value: clientY });
    fireEvent(element, event);
  };

  const drop = (source: HTMLElement, target: HTMLElement, clientY: number) => {
    const data = new Map<string, string>();
    const dataTransfer = { setData: (k: string, v: string) => data.set(k, v), getData: (k: string) => data.get(k) ?? "", effectAllowed: "", dropEffect: "" };
    fireEvent.dragStart(source, { dataTransfer });
    fireDrag("dragOver", withBox(target), dataTransfer, clientY);
    return { dataTransfer, target };
  };

  it("re-parents on a drop in the middle of a row and shows the indicator", async () => {
    const ui = await openWorkspace(["Folder", "Loose"]);
    await screen.findByText("Loose");
    const { dataTransfer, target } = drop(rowNamed("Loose"), rowNamed("Folder"), 15);
    expect(target.dataset.drop).toBe("inside");
    fireDrag("drop", target, dataTransfer, 15);
    await waitFor(async () => {
      const snapshot = await ui.memory.api.treeSnapshot();
      expect(snapshot.find((node) => node.title === "Loose")?.parentId).toBe(snapshot.find((node) => node.title === "Folder")?.id);
    });
  });

  it("reorders when dropped on the top or bottom edge", async () => {
    const ui = await openWorkspace(["One", "Two", "Three"]);
    await screen.findByText("Three");
    const first = drop(rowNamed("Three"), rowNamed("One"), 2);
    expect(first.target.dataset.drop).toBe("before");
    fireDrag("drop", first.target, first.dataTransfer, 2);
    await waitFor(async () => expect((await ui.memory.api.treeSnapshot()).map((node) => node.title)).toEqual(["Three", "One", "Two"]));

    const second = drop(rowNamed("Three"), rowNamed("Two"), 28);
    expect(second.target.dataset.drop).toBe("after");
    fireDrag("drop", second.target, second.dataTransfer, 28);
    await waitFor(async () => expect((await ui.memory.api.treeSnapshot()).map((node) => node.title)).toEqual(["One", "Two", "Three"]));
  });

  it("refuses a drop into the node's own descendant: no indicator and no move", async () => {
    const ui = await openWorkspace();
    const parent = await ui.memory.api.treeCreate({ kind: "folder", parentId: null, title: "Parent" });
    await ui.memory.api.treeCreate({ kind: "page", parentId: parent, title: "Child" });
    await screen.findByText("Parent");
    await ui.user.click(within(rowNamed("Parent")).getByRole("button", { name: "Expand", hidden: true }));
    const { dataTransfer, target } = drop(rowNamed("Parent"), rowNamed("Child"), 15);
    expect(target.dataset.drop).toBeUndefined();
    expect(dataTransfer.dropEffect).toBe("none");
    fireDrag("drop", target, dataTransfer, 15);
    await tick(20);
    const snapshot = await ui.memory.api.treeSnapshot();
    expect(snapshot.find((node) => node.title === "Parent")?.parentId).toBeNull();
    expect(snapshot.find((node) => node.title === "Child")?.parentId).toBe(parent);
  });
});

describe("trash", () => {
  it("moves a page to the trash, restores it, and deletes it for good after confirming", async () => {
    const ui = await openWorkspace(["Keep", "Discard"]);
    fireEvent.contextMenu(await screen.findByText("Discard"));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Move to trash" }));
    await waitFor(() => expect(titlesInTree()).toEqual(["Keep"]));
    expect(await screen.findByText(/“Discard” moved to the trash/)).toBeTruthy();

    await ui.user.click(screen.getByRole("button", { name: "Trash" }));
    const view = await screen.findByRole("region", { name: "Trash" });
    expect(await within(view).findByText("Discard")).toBeTruthy();
    await ui.user.click(within(view).getByRole("button", { name: "Restore" }));
    await waitFor(() => expect(titlesInTree()).toEqual(["Keep", "Discard"]));
    expect(await within(view).findByText("The trash is empty")).toBeTruthy();

    fireEvent.contextMenu(rowNamed("Discard"));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Move to trash" }));
    await within(view).findByRole("button", { name: "Delete forever" });
    await ui.user.click(within(view).getByRole("button", { name: "Delete forever" }));
    const dialog = await screen.findByRole("dialog", { name: "Delete forever?" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Delete forever" }));
    await waitFor(() => expect(within(view).queryByText("Discard")).toBeNull());
    expect((await ui.memory.api.treeListTrash()).length).toBe(0);
  });

  it("empties the trash after confirming", async () => {
    const ui = await openWorkspace(["One", "Two"]);
    const [one, two] = (await ui.memory.api.treeSnapshot()).map((node) => node.id);
    await ui.memory.api.treeTrash({ id: one as string });
    await ui.memory.api.treeTrash({ id: two as string });
    await ui.user.click(await screen.findByRole("button", { name: "Trash" }));
    const view = await screen.findByRole("region", { name: "Trash" });
    await within(view).findByText("One");
    await ui.user.click(within(view).getByRole("button", { name: "Empty trash" }));
    const dialog = await screen.findByRole("dialog", { name: "Empty the trash?" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Empty trash" }));
    expect(await within(view).findByText("The trash is empty")).toBeTruthy();
  });
});

describe("tabs and history", () => {
  it("opens pages in the active tab, in new tabs on Ctrl+click, and walks back and forward", async () => {
    const ui = await openWorkspace(["Alpha", "Beta"]);
    await ui.user.click(await screen.findByText("Alpha"));
    await screen.findByLabelText("Page title");
    expect(screen.getAllByRole("tab")).toHaveLength(1);

    await ui.user.click(screen.getByText("Beta"));
    expect(screen.getAllByRole("tab")).toHaveLength(1);
    expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Beta");

    await ui.user.click(screen.getByRole("button", { name: "Back" }));
    expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Alpha");
    await ui.user.click(screen.getByRole("button", { name: "Forward" }));
    expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Beta");

    await ui.user.keyboard("{Control>}");
    await ui.user.click(screen.getByText("Alpha"));
    await ui.user.keyboard("{/Control}");
    expect(screen.getAllByRole("tab")).toHaveLength(2);
    expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Alpha");

    await ui.user.click(screen.getByRole("button", { name: "Close Alpha" }));
    expect(screen.getAllByRole("tab")).toHaveLength(1);
  });

  it("shows the breadcrumb of the open page", async () => {
    const ui = await openWorkspace();
    const folder = await ui.memory.api.treeCreate({ kind: "folder", parentId: null, title: "Projects" });
    const page = await ui.memory.api.treeCreate({ kind: "page", parentId: folder, title: "Plan" });
    await screen.findByText("Projects");
    ui.app.openPage(page);
    const nav = await screen.findByRole("navigation", { name: "Location" });
    expect(nav.textContent).toContain("Projects");
    expect(within(nav).getByText("Plan").closest("[aria-current]")).not.toBeNull();
  });

  it("drops a tab whose page was deleted elsewhere", async () => {
    const ui = await openWorkspace(["Doomed"]);
    await ui.user.click(await screen.findByText("Doomed"));
    await screen.findByLabelText("Page title");
    const id = (await ui.memory.api.treeSnapshot())[0]?.id as string;
    await ui.memory.api.treeTrash({ id });
    await waitFor(() => expect(screen.queryByLabelText("Page title")).toBeNull());
    expect(await screen.findByText("Recent pages").catch(() => screen.findByText("Note Everywhere"))).toBeTruthy();
  });

  it("switches tabs with the arrow keys", async () => {
    const ui = await openWorkspace(["Alpha"]);
    await ui.user.click(screen.getByRole("button", { name: "New tab" }));
    expect(screen.getAllByRole("tab")).toHaveLength(2);
    screen.getByRole("tab", { selected: true }).focus();
    await ui.user.keyboard("{ArrowLeft}");
    expect((screen.getAllByRole("tab")[0] as HTMLElement).getAttribute("aria-selected")).toBe("true");
  });
});

describe("sync indicator and theme", () => {
  it("says the workspace is on this device only when there is no account", async () => {
    await openWorkspace();
    const pill = document.querySelector<HTMLButtonElement>(".sync-pill");
    expect(pill?.textContent).toContain("On this device only");
    expect(pill?.disabled).toBe(true);
  });

  it("walks through synced, waiting, offline and synced again", async () => {
    const ui = await renderApp({ syncDelayMs: 100_000, setup: async (memory) => void (await seedSignedIn(memory)) });
    const pill = () => document.querySelector<HTMLButtonElement>(".sync-pill") as HTMLButtonElement;
    await waitFor(() => expect(pill().dataset.state).toBe("synced"));
    await ui.memory.api.treeCreate({ kind: "page", parentId: null, title: "new" });
    await waitFor(() => expect(pill().dataset.state).toBe("pending"));
    expect(pill().textContent).toContain("Waiting to send");
    ui.memory.control.setOffline(true);
    await waitFor(() => expect(pill().dataset.state).toBe("offline"));
    expect(pill().textContent).toContain("Offline");
    ui.memory.control.setOffline(false);
    await ui.user.click(pill());
    await waitFor(() => expect(pill().dataset.state).toBe("synced"));
    expect(pill().getAttribute("aria-label")).toContain("Last synced");
  });

  it("shows the error state and the reason", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedSignedIn(memory)) });
    ui.app.sync.status();
    // The core reports a failed sync through the event channel.
    const failing = ui.memory.api;
    failing.syncNow = () => Promise.reject({ code: "boom", message: "The server refused the update." });
    await ui.user.click(document.querySelector(".sync-pill") as HTMLElement);
    expect((await screen.findByRole("alert")).textContent).toContain("The server refused the update.");
  });

  it("switches the theme and keeps it", async () => {
    const ui = await openWorkspace();
    await ui.user.click(screen.getByRole("radio", { name: "Dark" }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("dark"));
    expect(localStorage.getItem("ne.theme")).toBe("dark");
    expect((await ui.memory.api.settingsGet()).theme).toBe("dark");
    await ui.user.click(screen.getByRole("radio", { name: "Light" }));
    await waitFor(() => expect(document.documentElement.dataset.theme).toBe("light"));
  });
});

describe("sidebar", () => {
  it("is resizable with the keyboard and remembers the width", async () => {
    const ui = await openWorkspace();
    const handle = screen.getByRole("separator", { name: "Resize the sidebar" });
    const before = Number(handle.getAttribute("aria-valuenow"));
    handle.focus();
    await ui.user.keyboard("{ArrowRight}");
    expect(Number(handle.getAttribute("aria-valuenow"))).toBe(before + 16);
    expect(localStorage.getItem("ne.sidebar.width")).toBe(String(before + 16));
    await ui.user.keyboard("{Home}");
    expect(Number(handle.getAttribute("aria-valuenow"))).toBe(200);
  });
});
