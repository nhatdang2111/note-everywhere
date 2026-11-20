import { LoroDoc, LoroList, LoroMap, LoroText } from "loro-crdt";
import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import { pressKey, setParagraph, typeInto } from "../test/editor-harness.ts";
import { renderApp, seedLocal, tick, type RenderedApp } from "../test/app-harness.tsx";
import { cellAt, columnOf, renderDatabase } from "./database/database-harness.tsx";

async function openPage(options: Parameters<typeof renderApp>[0] = {}, titles = ["Alpha", "Beta"]) {
  const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory, titles)), ...options });
  const [first] = await ui.memory.api.treeSnapshot();
  await screen.findByRole("navigation", { name: "Workspace" });
  ui.app.openPage(first?.id as string);
  const editor = await waitFor(() => ui.app.ui.activeEditor() ?? Promise.reject(new Error("editor not ready")));
  return { ui, editor, pageId: first?.id as string, view: editor.view };
}

const menuItem = (name: string | RegExp) => screen.findByRole("menuitem", { name });

describe("creating databases", () => {
  it("makes a database from the sidebar and from the slash menu as a child node, and keeps rows out of the sidebar", async () => {
    const { ui, pageId } = await openPage();
    await ui.user.click(screen.getByRole("button", { name: "New database" }));
    await waitFor(() => expect(ui.app.tree.nodes().some((node) => node.kind === "database")).toBe(true));
    await screen.findByLabelText("Database title");
    ui.app.openPage(pageId);
    const editor = await waitFor(() => {
      const current = ui.app.ui.activeEditor();
      if (!current || current.view.dom.closest("article")?.getAttribute("aria-label") !== "Alpha") throw new Error("editor not ready");
      return current;
    });
    setParagraph(editor.view, "");
    typeInto(editor.view, "/datab");
    const list = await screen.findByRole("listbox", { name: "Insert a block" });
    expect(within(list).getAllByRole("option")).toHaveLength(1);
    pressKey(editor.view, "Enter");
    await waitFor(() => expect(ui.app.tree.nodes().filter((node) => node.kind === "database" && node.parentId === pageId)).toHaveLength(1));
    const child = ui.app.tree.nodes().find((node) => node.kind === "database" && node.parentId === pageId);
    await waitFor(() => {
      const kinds: string[] = [];
      editor.view.state.doc.forEach((node) => kinds.push(node.type.name === "child_page" ? String(node.attrs.pageId) : node.type.name));
      expect(kinds).toContain(child?.id);
    });
    await ui.memory.api.dbRowCreate({ id: child?.id as string, title: "Hidden row" });
    await ui.app.tree.refresh();
    expect(within(screen.getByRole("tree")).queryByText("Hidden row")).toBeNull();
  });
});

describe("templates", () => {
  it("saves a page as a template and starts a new page from the gallery", async () => {
    const { ui, pageId } = await openPage();
    await ui.user.click(screen.getByRole("button", { name: "Save as template…" }));
    const dialog = await screen.findByRole("dialog", { name: "Save page as template" });
    const name = within(dialog).getByLabelText("Template name") as HTMLInputElement;
    expect(name.value).toBe("Alpha");
    await ui.user.clear(name);
    await ui.user.type(name, "Weekly{Enter}");
    await waitFor(async () => expect((await ui.memory.api.templateList({})).map((t) => t.name)).toEqual(["Weekly"]));

    await ui.user.click(screen.getByRole("button", { name: "Workspace menu" }));
    await ui.user.click(await menuItem("New page from template…"));
    const gallery = await screen.findByRole("dialog", { name: "New page from a template" });
    expect(within(gallery).getByText("Blank page")).toBeTruthy();
    await ui.user.click(await within(gallery).findByRole("button", { name: /^Weekly/ }));
    await waitFor(async () => expect((await ui.memory.api.treeSnapshot()).filter((n) => n.title === "Alpha")).toHaveLength(2));
    await waitFor(() => expect(ui.app.tabs.activePageId()).not.toBe(pageId));
  });

  it("renames and deletes templates in the gallery", async () => {
    const { ui, pageId } = await openPage();
    await ui.memory.api.templateCreateFromPage({ pageId, name: "Old name" });
    await ui.user.click(screen.getByRole("button", { name: "Workspace menu" }));
    await ui.user.click(await menuItem("New page from template…"));
    const gallery = await screen.findByRole("dialog", { name: "New page from a template" });
    await ui.user.click(await within(gallery).findByRole("button", { name: "Rename Old name" }));
    const input = within(gallery).getByLabelText("Template name");
    await ui.user.clear(input);
    await ui.user.type(input, "New name{Enter}");
    await waitFor(async () => expect((await ui.memory.api.templateList({}))[0]?.name).toBe("New name"));
    await ui.user.click(await within(gallery).findByRole("button", { name: "Delete New name" }));
    await waitFor(async () => expect(await ui.memory.api.templateList({})).toEqual([]));
    await within(gallery).findByText("No templates yet");
  });

  it("gives a database row templates with default properties that new rows start with", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(screen.getAllByRole("gridcell").length).toBeGreaterThan(3));
    await ui.user.click(screen.getByRole("button", { name: "New row from a template" }));
    await ui.user.click(await menuItem("Manage templates…"));
    const dialog = await screen.findByRole("dialog", { name: "Row templates" });
    await ui.user.click(within(dialog).getByRole("button", { name: "New template" }));
    await waitFor(async () => expect(await ui.memory.api.templateList({ databaseId: sample.db })).toHaveLength(1));
    const name = await within(dialog).findByLabelText("Template name");
    await ui.user.clear(name);
    await ui.user.type(name, "Bug{Enter}");
    await waitFor(async () => expect((await ui.memory.api.templateList({ databaseId: sample.db }))[0]?.name).toBe("Bug"));
    await ui.user.click(within(dialog).getAllByText("Empty")[0] as HTMLElement);
    await ui.user.click(await screen.findByRole("option", { name: "Done" }));
    await waitFor(async () => expect((await ui.memory.api.templateList({ databaseId: sample.db }))[0]?.defaults[sample.status]).toEqual({ type: "select", value: sample.done }));
    await ui.user.click(within(dialog).getByRole("button", { name: "Done" }));
    await ui.user.click(screen.getByRole("button", { name: "New row from a template" }));
    await ui.user.click(await menuItem("Bug"));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).total).toBe(4));
    expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[3]?.values[sample.status]).toEqual({ type: "select", value: sample.done });
    await waitFor(() => expect(within(cellAt(3, columnOf(sample, sample.status))).getByText("Done")).toBeTruthy());
  });
});

describe("page history", () => {
  async function editAs(ui: RenderedApp, pageId: string, at: number, word: string) {
    const peer = new LoroDoc();
    peer.import((await ui.memory.api.pageOpen({ id: pageId })).snapshot);
    peer.setRecordTimestamp(true);
    peer.setChangeMergeInterval(0);
    peer.setNextCommitTimestamp(at);
    const children = peer.getMap("doc").get("children") as LoroList;
    const paragraph = children.insertContainer(children.length, new LoroMap());
    paragraph.set("nodeName", "paragraph");
    paragraph.setContainer("attributes", new LoroMap());
    paragraph.setContainer("children", new LoroList()).insertContainer(0, new LoroText()).insert(0, word);
    peer.commit();
    // Arrives like an edit from another device, so the open editor has it too.
    ui.memory.control.receiveRemoteUpdate(pageId, peer.export({ mode: "snapshot" }));
  }

  it("lists versions, previews one read-only, and restores it after confirming", async () => {
    const { ui, editor, pageId } = await openPage();
    const base = Math.floor(Date.now() / 1000) - 7200;
    await editAs(ui, pageId, base, "first draft");
    await editAs(ui, pageId, base + 3600, "second thought");
    await ui.user.click(screen.getByRole("button", { name: "Page history" }));
    const panel = await screen.findByRole("complementary", { name: "Page history" });
    const versions = await within(panel).findAllByRole("button", { pressed: false });
    expect(versions.length).toBeGreaterThanOrEqual(3);
    const old = versions.find((b) => b.textContent?.includes("1 changes"));
    expect(old).toBeTruthy();
    // The oldest edited version shows the first draft only.
    const list = within(panel).getByRole("list", { name: "Versions" });
    const buttons = within(list).getAllByRole("button");
    const target = buttons[buttons.length - 2] as HTMLElement;
    await ui.user.click(target);
    const preview = await within(panel).findByRole("region", { name: "Preview" });
    await within(panel).findByText("first draft");
    expect(within(panel).getByRole("region", { name: "Preview" }).textContent).not.toContain("second thought");
    expect(within(preview).queryByRole("textbox")).toBeNull();
    await ui.user.click(within(panel).getByRole("button", { name: "Restore this version" }));
    const confirm = await screen.findByRole("dialog", { name: "Restore this version?" });
    await ui.user.click(within(confirm).getByRole("button", { name: "Restore this version" }));
    await waitFor(() => expect(editor.view.state.doc.textContent).toContain("first draft"));
    expect(editor.view.state.doc.textContent).not.toContain("second thought");
    await screen.findByText("The page was restored.");
  });

  it("says plainly when older history is unavailable and offers no preview", async () => {
    const { ui, pageId } = await openPage();
    await editAs(ui, pageId, Math.floor(Date.now() / 1000), "x");
    ui.memory.control.setHistoryTrimmed(pageId, true);
    await ui.user.click(screen.getByRole("button", { name: "Page history" }));
    const panel = await screen.findByRole("complementary", { name: "Page history" });
    await within(panel).findByText(/Older history is not available for this page/);
    for (const version of within(panel).getAllByRole("button", { pressed: false })) expect((version as HTMLButtonElement).disabled).toBe(true);
  });
});

describe("structured import", () => {
  it("shows the report, including the items that were skipped", async () => {
    const file = (path: string, content: string) => {
      const made = new File([content], path.split("/").pop() as string);
      Object.defineProperty(made, "webkitRelativePath", { value: path });
      return made;
    };
    const ui = await renderApp({
      setup: async (memory) => void (await seedLocal(memory)),
      pickFiles: async () => [file("Export/Plan.md", "# Plan\n[Gone](Missing.md)"), file("Export/Tasks.csv", "Name,Points\nA,1\nB,2\n"), file("Export/odd.bin", "x")],
    });
    await screen.findByRole("navigation", { name: "Workspace" });
    await ui.user.click(screen.getByRole("button", { name: "Workspace menu" }));
    expect(await menuItem("Import Markdown…")).toBeTruthy();
    await ui.user.click(await menuItem("Import Markdown & CSV export folder…"));
    const dialog = await screen.findByRole("dialog", { name: "Import a Markdown & CSV export folder" });
    await ui.user.click(within(dialog).getByRole("button", { name: "Choose folder…" }));
    const report = await within(dialog).findByText("Database rows");
    expect(report.nextElementSibling?.textContent).toBe("2");
    expect(within(dialog).getByText("Links to pages not in the export").closest(".report-row")?.getAttribute("data-warn")).toBe("true");
    expect(within(dialog).getByText("Skipped (1)")).toBeTruthy();
    expect(within(dialog).getByText(/odd\.bin: unsupported file type/)).toBeTruthy();
    await waitFor(() => expect(ui.app.tree.nodes().map((n) => n.title)).toEqual(["Plan", "Tasks"]));
  });
});

describe("synced blocks in a page", () => {
  it("creates a synced block from the selected blocks through the block menu, with a confirmation", async () => {
    const { ui, editor, view, pageId } = await openPage();
    setParagraph(view, "to share");
    await waitFor(() => expect(JSON.stringify(ui.memory.control.pageDoc(pageId)?.getMap("doc").toJSON())).toContain("to share"));
    await tick(40);
    pressKey(editor.view, "/", { ctrlKey: true });
    await ui.user.click(await menuItem("Create synced block"));
    const confirm = await screen.findByRole("dialog", { name: "Create a synced block?" });
    await ui.user.click(within(confirm).getByRole("button", { name: "Create" }));
    await waitFor(() => expect(editor.view.state.doc.child(0).type.name).toBe("synced_block"));
    const frame = await waitFor(() => editor.view.dom.querySelector<HTMLElement>(".ne-synced[data-state='ready']") ?? Promise.reject(new Error("not ready")));
    expect(frame.getAttribute("aria-label")).toBe("Synced block");
    await waitFor(() => expect(frame.textContent).toContain("to share"));
    expect(await ui.memory.api.syncedList()).toHaveLength(1);
    // Unsync from the block's own menu.
    await ui.user.click(within(frame).getByRole("button", { name: "Synced block options" }));
    await ui.user.click(await menuItem("Unsync"));
    await ui.user.click(within(await screen.findByRole("dialog", { name: "Unsync this block?" })).getByRole("button", { name: "Unsync" }));
    await waitFor(() => expect(editor.view.state.doc.child(0).type.name).toBe("paragraph"));
    expect(editor.view.state.doc.textContent).toContain("to share");
  });
});

describe("reminders", () => {
  it("sets a reminder on a page and lists it with Done and Snooze", async () => {
    const { ui, pageId } = await openPage();
    await ui.user.click(screen.getByRole("button", { name: "Set a reminder" }));
    const dialog = await screen.findByRole("dialog", { name: "Remind me about Alpha" });
    fireEvent.input(within(dialog).getByLabelText("Date and time"), { target: { value: "2031-03-04T09:30" } });
    await ui.user.type(within(dialog).getByLabelText("Note (optional)"), "bring slides");
    await ui.user.click(within(dialog).getByRole("button", { name: "Set reminder" }));
    await waitFor(async () => expect((await ui.memory.api.reminderList({})).map((e) => [e.reminder.pageId, e.reminder.note, e.dueAt]).flat()).toEqual([pageId, "bring slides", new Date("2031-03-04T09:30").getTime()]));
    await ui.user.click(screen.getByRole("button", { name: "Reminders" }));
    const panel = await screen.findByRole("complementary", { name: "Reminders" });
    const item = await within(panel).findByText("bring slides");
    expect(within(panel).getByRole("button", { name: "Alpha" })).toBeTruthy();
    await ui.user.click(within(item.closest("li") as HTMLElement).getByRole("button", { name: "Snooze" }));
    await ui.user.click(await menuItem("In 1 hour"));
    await waitFor(async () => expect((await ui.memory.api.reminderList({}))[0]?.reminder.snoozedUntil).toBeGreaterThan(Date.now()));
    await ui.user.click(within(panel).getByRole("button", { name: "Done" }));
    await within(panel).findByText("No reminders");
    await ui.user.click(within(panel).getByRole("checkbox", { name: "Show completed" }));
    await within(panel).findByRole("list", { name: "Completed reminders" });
    await ui.user.click(within(panel).getByRole("button", { name: "Reopen" }));
    await waitFor(() => expect(within(panel).queryByRole("list", { name: "Completed reminders" })).toBeNull());
    await ui.user.click(within(panel).getByRole("button", { name: "Delete reminder" }));
    await within(panel).findByText("No reminders");
  });

  it("sets a reminder that follows a date property of a row", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(screen.getAllByRole("gridcell").length).toBeGreaterThan(3));
    await ui.user.click(screen.getByRole("button", { name: "Open Ship release" }));
    const properties = await screen.findByRole("region", { name: "Properties" });
    await ui.user.click(within(properties).getByRole("button", { name: "On the date of Due" }));
    const dialog = await screen.findByRole("dialog", { name: /Remind me about Ship release/ });
    expect((within(dialog).getByRole("radio", { name: "On the date of Due" }) as HTMLInputElement).checked).toBe(true);
    await ui.user.click(within(dialog).getByRole("button", { name: "Set reminder" }));
    await waitFor(async () => expect((await ui.memory.api.reminderList({}))[0]?.reminder.propertyId).toBe(sample.due));
  });

  it("shows an in-app toast when a reminder is due, with a way to open the page", async () => {
    const { ui, pageId } = await openPage();
    await ui.memory.api.reminderAdd({ pageId: (await ui.memory.api.treeSnapshot())[1]?.id as string, at: Date.now() + 5000, utcOffsetMinutes: 0, note: "now" });
    ui.memory.control.advanceClock(6000);
    const toast = await screen.findByText(/Reminder: Beta/);
    expect(toast.closest('[role="status"]')).not.toBeNull();
    await ui.user.click(screen.getByRole("button", { name: "Open page" }));
    await waitFor(() => expect(ui.app.tabs.activePageId()).not.toBe(pageId));
  });

  it("shows no second message when the core already showed a native notification", async () => {
    const { ui } = await openPage({ nativeReminders: true });
    await ui.memory.api.reminderAdd({ pageId: (await ui.memory.api.treeSnapshot())[1]?.id as string, at: Date.now() + 5000, utcOffsetMinutes: 0 });
    ui.memory.control.advanceClock(6000);
    await tick(50);
    expect(screen.queryByText(/Reminder: Beta/)).toBeNull();
    // The reminder is still listed as due.
    await waitFor(() => expect(ui.app.reminders.entries()).toHaveLength(1));
  });
});

describe("updates", () => {
  const openSettings = async (ui: RenderedApp) => {
    await ui.user.click(await screen.findByRole("button", { name: "Settings" }));
    return screen.findByRole("region", { name: "Updates" });
  };

  it("checks, downloads with progress, and installs only after a confirmation", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    await screen.findByRole("navigation", { name: "Workspace" });
    ui.memory.control.announceUpdate("2.0.0", "Faster tables");
    const section = await openSettings(ui);
    await within(section).findByText(/No check has run yet/);
    expect(screen.queryByRole("button", { name: /Update available/ })).toBeNull();
    await ui.user.click(within(section).getByRole("button", { name: "Check for updates" }));
    await within(section).findByText(/A new version is available/);
    expect(within(section).getByText(/Version 2\.0\.0/)).toBeTruthy();
    expect(within(section).getByText("Faster tables")).toBeTruthy();
    expect(screen.getByRole("button", { name: "Update available" })).toBeTruthy();
    expect(within(section).queryByRole("button", { name: "Install and restart" })).toBeNull();
    await ui.user.click(within(section).getByRole("button", { name: "Download update" }));
    await within(section).findByText(/downloaded and checked/);
    expect(screen.getByRole("button", { name: "Update ready" })).toBeTruthy();
    let installed = 0;
    const original = ui.memory.api.updateInstall;
    ui.memory.api.updateInstall = async () => {
      installed += 1;
      return original();
    };
    await ui.user.click(within(section).getByRole("button", { name: "Install and restart" }));
    const confirm = await screen.findByRole("dialog", { name: "Install and restart?" });
    expect(installed).toBe(0);
    await ui.user.click(within(confirm).getByRole("button", { name: "Cancel" }));
    expect(installed).toBe(0);
    await ui.user.click(within(section).getByRole("button", { name: "Install and restart" }));
    await ui.user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Install and restart" }));
    await waitFor(() => expect(installed).toBe(1));
  });

  it("says when the app is up to date and shows download progress while downloading", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    await screen.findByRole("navigation", { name: "Workspace" });
    const section = await openSettings(ui);
    await ui.user.click(within(section).getByRole("button", { name: "Check for updates" }));
    await within(section).findByText("You have the newest version.");
    expect(within(section).getByText(/Last checked/)).toBeTruthy();
    expect(within(section).queryByRole("button", { name: "Download update" })).toBeNull();
    // Events from the core drive the states without a click.
    ui.memory.control.announceUpdate("2.1.0");
    await ui.user.click(within(section).getByRole("button", { name: "Check for updates" }));
    await within(section).findByRole("button", { name: "Download update" });
  });

  it("shows only the reason when this build cannot update itself", async () => {
    const ui = await renderApp({ updatesUnavailable: "This build has no update key.", setup: async (memory) => void (await seedLocal(memory)) });
    await screen.findByRole("navigation", { name: "Workspace" });
    const section = await openSettings(ui);
    await within(section).findByText("This build cannot update itself.");
    expect(within(section).getByText("This build has no update key.")).toBeTruthy();
    expect(within(section).queryAllByRole("button")).toHaveLength(0);
    expect(screen.queryByRole("button", { name: /Update/ })).toBeNull();
  });

  it("reports a failed check as a state, not a crash", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    await screen.findByRole("navigation", { name: "Workspace" });
    const section = await openSettings(ui);
    ui.memory.api.updateCheck = async () => ({ state: "error", currentVersion: "0.0.0-preview", lastCheckedAt: null, message: "Cannot reach the update server." });
    await ui.user.click(within(section).getByRole("button", { name: "Check for updates" }));
    await within(section).findByText("The last check failed.");
    expect(within(section).getByText("Cannot reach the update server.")).toBeTruthy();
  });
});

describe("voice recordings", () => {
  it("adds the recording as an attachment block after the text when keeping audio is on", async () => {
    const { ui, view } = await openPage();
    await ui.memory.api.voiceDownloadModel({ name: "small-q5" });
    await ui.app.voice.refresh();
    await ui.memory.api.settingsSet({ settings: { voiceKeepAudio: true } });
    setParagraph(view, "");
    await tick(40);
    ui.memory.control.setNextTranscript("hello there");
    ui.memory.control.pressHotkey(true);
    await screen.findByText("Recording… release to insert");
    ui.memory.control.pressHotkey(false);
    await waitFor(() => expect(view.state.doc.textContent).toContain("hello there"));
    const blocks: { text: string; href: string | null }[] = [];
    view.state.doc.forEach((node) => blocks.push({ text: node.textContent, href: (node.firstChild?.marks.find((m) => m.type.name === "link")?.attrs.href as string) ?? null }));
    expect(blocks[0]?.text).toBe("hello there");
    expect(blocks[1]).toMatchObject({ text: "Voice recording" });
    expect(blocks[1]?.href).toMatch(/^attachment:/);
  });

  it("inserts only the text when the setting is off", async () => {
    const { ui, view } = await openPage();
    await ui.memory.api.voiceDownloadModel({ name: "small-q5" });
    await ui.app.voice.refresh();
    setParagraph(view, "");
    await tick(40);
    ui.memory.control.setNextTranscript("just words");
    ui.memory.control.pressHotkey(true);
    await screen.findByText("Recording… release to insert");
    ui.memory.control.pressHotkey(false);
    await waitFor(() => expect(view.state.doc.textContent).toBe("just words"));
    expect(view.state.doc.childCount).toBe(1);
  });
});
