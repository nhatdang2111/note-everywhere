import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import { dayText } from "../../lib/db/calendar-model.ts";
import { cellAt, columnOf, renderDatabase } from "./database-harness.tsx";
import type { RenderedApp } from "../../test/app-harness.tsx";

const rowTitles = () => [...document.querySelectorAll('[role="gridcell"][data-col="0"] .cell-text')].map((el) => el.textContent);
const views = async (ui: RenderedApp, db: string) => (await ui.memory.api.dbSchema({ id: db })).views;

async function addView(ui: RenderedApp, kind: "Table" | "Board" | "List" | "Calendar") {
  await ui.user.click(screen.getByRole("button", { name: "Add a view" }));
  await ui.user.click(await screen.findByRole("menuitem", { name: kind }));
}

describe("database views", () => {
  it("adds, renames, duplicates, reorders and deletes views", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    await addView(ui, "Board");
    await waitFor(async () => expect((await views(ui, sample.db)).map((v) => v.kind)).toEqual(["table", "board"]));
    expect(screen.getByRole("tab", { name: /Board/ }).getAttribute("aria-selected")).toBe("true");

    await ui.user.dblClick(screen.getByRole("tab", { name: /Board/ }));
    const rename = screen.getByLabelText("Rename view");
    await ui.user.clear(rename);
    await ui.user.type(rename, "Kanban{Enter}");
    await waitFor(async () => expect((await views(ui, sample.db)).map((v) => v.name)).toEqual(["Table", "Kanban"]));

    await ui.user.click(screen.getAllByRole("button", { name: "View options" })[0] as HTMLElement);
    await ui.user.click(await screen.findByRole("menuitem", { name: "Duplicate view" }));
    await waitFor(async () => expect((await views(ui, sample.db)).map((v) => v.name)).toEqual(["Table", "Kanban", "Kanban (copy)"]));
    expect((await views(ui, sample.db))[2]?.group?.property).toBe(sample.status);

    await ui.user.click(screen.getAllByRole("button", { name: "View options" })[0] as HTMLElement);
    await ui.user.click(await screen.findByRole("menuitem", { name: "Move view left" }));
    await waitFor(async () => expect((await views(ui, sample.db)).map((v) => v.name)).toEqual(["Table", "Kanban (copy)", "Kanban"]));

    // Arrow keys move between tabs.
    screen.getByRole("tab", { name: /Kanban \(copy\)/ }).focus();
    await ui.user.keyboard("{ArrowLeft}");
    await waitFor(() => expect(screen.getByRole("tab", { name: /Table/ }).getAttribute("aria-selected")).toBe("true"));

    await ui.user.click(screen.getByRole("tab", { name: /Kanban \(copy\)/ }));
    await ui.user.click(screen.getAllByRole("button", { name: "View options" })[0] as HTMLElement);
    await ui.user.click(await screen.findByRole("menuitem", { name: "Delete view" }));
    await ui.user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(async () => expect((await views(ui, sample.db)).map((v) => v.name)).toEqual(["Table", "Kanban"]));
  });

  it("filters with the builder and saves the filter with the view", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await ui.user.click(screen.getByRole("button", { name: /^Filter/ }));
    const panel = await screen.findByRole("dialog", { name: "Filter" });
    await ui.user.click(within(panel).getByRole("button", { name: "Add condition" }));
    const property = within(panel).getByLabelText("Property");
    expect(within(property).getAllByRole("option").map((o) => o.textContent)).toContain("Points");
    fireEvent.change(within(panel).getByLabelText("Condition"), { target: { value: "contains" } });
    await ui.user.type(within(panel).getByLabelText("Value"), "ship");
    await waitFor(async () => expect((await views(ui, sample.db))[0]?.filter).toMatchObject({ children: [{ value: "ship" }] }));
    await waitFor(() => expect(rowTitles()).toEqual(["Ship release"]));
    expect((await views(ui, sample.db))[0]?.filter).toEqual({ type: "group", logic: "and", children: [{ type: "condition", property: "title", operator: "contains", value: "ship" }] });
    expect(screen.getByRole("button", { name: /^Filter/ }).textContent).toContain("1");

    // Switch the property: the operator list follows the type, and a valueless operator needs no value.
    fireEvent.change(property, { target: { value: sample.flag } });
    const operators = within(within(panel).getByLabelText("Condition")).getAllByRole("option").map((o) => o.textContent);
    expect(operators).toEqual(["is checked", "is not checked"]);
    await waitFor(() => expect(rowTitles()).toHaveLength(0));
    fireEvent.change(within(panel).getByLabelText("Condition"), { target: { value: "is_unchecked" } });
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    expect(within(panel).queryByLabelText("Value")).toBeNull();

    // A nested group, with or.
    await ui.user.click(within(panel).getByRole("button", { name: "Add group" }));
    expect(within(panel).getAllByRole("group")).toHaveLength(2);
    await ui.user.click(within(panel).getAllByRole("button", { name: "Remove group" })[0] as HTMLElement);
    await ui.user.click(within(panel).getByRole("button", { name: "Remove condition" }));
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    expect((await views(ui, sample.db))[0]?.filter).toBeNull();
  });

  it("sorts by several keys, groups, and picks the visible properties", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await ui.user.click(screen.getByRole("button", { name: /^Sort/ }));
    const panel = await screen.findByRole("dialog", { name: "Sort" });
    await ui.user.click(within(panel).getByRole("button", { name: "Add sort" }));
    fireEvent.change(within(panel).getByLabelText("Direction"), { target: { value: "desc" } });
    await waitFor(() => expect(rowTitles()).toEqual(["Write report", "Ship release", "Plan trip"]));
    await ui.user.click(within(panel).getByRole("button", { name: "Add sort" }));
    expect((await views(ui, sample.db))[0]?.sorts).toEqual([{ property: "title", descending: true }, { property: sample.status, descending: false }]);
    await ui.user.click(within(panel).getAllByRole("button", { name: "Remove sort" })[0] as HTMLElement);
    await waitFor(() => expect((screen.getAllByRole("columnheader")[0] as HTMLElement).textContent).toContain("Title"));
    await ui.user.keyboard("{Escape}");

    await ui.user.click(screen.getByRole("button", { name: "Properties" }));
    const picker = await screen.findByRole("dialog", { name: "Properties" });
    await ui.user.click(within(picker).getByRole("checkbox", { name: "Points" }));
    await waitFor(() => expect(screen.queryByRole("columnheader", { name: /Points/ })).toBeNull());
    await ui.user.click(within(picker).getByRole("button", { name: "Move Link up" }));
    await waitFor(async () => {
      const columns = (await views(ui, sample.db))[0]?.columns.map((c) => c.property);
      expect(columns?.indexOf(sample.link)).toBeLessThan(columns?.indexOf(sample.flag) ?? 0);
    });
    expect(columnOf(sample, sample.points)).toBeGreaterThan(0);
    // Sort from a column menu.
    await ui.user.keyboard("{Escape}");
    await ui.user.click(screen.getByRole("button", { name: "Status column menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Sort descending" }));
    await waitFor(async () => expect((await views(ui, sample.db))[0]?.sorts[0]).toEqual({ property: sample.status, descending: true }));
  });

  it("resizes and reorders columns, with the pointer or the keyboard", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    const handle = screen.getByRole("separator", { name: "Resize the Status column" });
    handle.focus();
    await ui.user.keyboard("{ArrowRight}{ArrowRight}");
    await waitFor(async () => expect((await views(ui, sample.db))[0]?.columns.find((c) => c.property === sample.status)?.width).toBe(180 + 32));
    expect(handle.getAttribute("aria-valuenow")).toBe("212");
    fireEvent.pointerDown(handle, { clientX: 100 });
    fireEvent.pointerMove(window, { clientX: 150 });
    expect(handle.getAttribute("aria-valuenow")).toBe("262");
    fireEvent.pointerUp(window);
    await waitFor(async () => expect((await views(ui, sample.db))[0]?.columns.find((c) => c.property === sample.status)?.width).toBe(262));

    await ui.user.click(screen.getByRole("button", { name: "Points column menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Move left" }));
    await waitFor(async () => {
      const order = (await views(ui, sample.db))[0]?.columns.map((c) => c.property) ?? [];
      expect(order.indexOf(sample.points)).toBeLessThan(order.indexOf(sample.tags));
    });
    // Drag a header onto another.
    const headers = screen.getAllByRole("columnheader");
    const flag = headers.find((h) => h.textContent?.includes("Flag")) as HTMLElement;
    const status = headers.find((h) => h.textContent?.includes("Status")) as HTMLElement;
    fireEvent.dragStart(flag);
    fireEvent.dragOver(status);
    fireEvent.drop(status);
    await waitFor(async () => {
      const order = (await views(ui, sample.db))[0]?.columns.map((c) => c.property) ?? [];
      expect(order.indexOf(sample.flag)).toBeLessThan(order.indexOf(sample.status));
    });
  });
});

describe("schema editing", () => {
  it("adds, renames and deletes a property", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await ui.user.click(screen.getByRole("button", { name: "Add a property" }));
    const dialog = await screen.findByRole("dialog", { name: "Add a property" });
    await ui.user.type(within(dialog).getByLabelText("Property name"), "Effort");
    fireEvent.change(within(dialog).getByLabelText("Type"), { target: { value: "number" } });
    await ui.user.click(within(dialog).getByRole("button", { name: "Add a property" }));
    await waitFor(() => expect(screen.getByRole("columnheader", { name: /Effort/ })).toBeTruthy());
    const added = (await ui.memory.api.dbSchema({ id: sample.db })).properties.at(-1);
    expect(added).toMatchObject({ name: "Effort", type: "number" });

    await ui.user.click(screen.getByRole("button", { name: "Effort column menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Rename property" }));
    const rename = within(await screen.findByRole("dialog")).getByLabelText("Property name");
    await ui.user.clear(rename);
    await ui.user.type(rename, "Hours{Enter}");
    await waitFor(() => expect(screen.getByRole("columnheader", { name: /Hours/ })).toBeTruthy());

    await ui.user.click(screen.getByRole("button", { name: "Hours column menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Delete property…" }));
    await ui.user.click(within(await screen.findByRole("dialog")).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(screen.queryByRole("columnheader", { name: /Hours/ })).toBeNull());
  });

  it("shows what a type change does to real values before it is confirmed, and keeps the values when changing back", async () => {
    const { ui, sample } = await renderDatabase({
      setup: async (memory) => {
        const [first] = (await memory.api.treeSnapshot()).filter((n) => n.kind === "database");
        const db = first?.id as string;
        const note = await memory.api.dbPropertyAdd({ id: db, name: "Note", type: "text" });
        const rows = (await memory.api.dbQuery({ id: db })).rows;
        await memory.api.dbCellSet({ id: db, rowId: rows[0]?.id as string, propertyId: note, value: { type: "text", value: "Urgent" } });
        await memory.api.dbCellSet({ id: db, rowId: rows[1]?.id as string, propertyId: note, value: { type: "text", value: "12" } });
      },
    });
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    const note = (await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.name === "Note")?.id as string;
    await ui.user.click(screen.getByRole("button", { name: "Note column menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Change type…" }));
    const dialog = await screen.findByRole("dialog", { name: /Change the type of “Note”/ });
    fireEvent.change(within(dialog).getByLabelText("Type"), { target: { value: "number" } });
    expect(within(dialog).getByRole("status").textContent).toContain("Values are read as the new type");
    const preview = within(dialog).getByRole("table");
    expect(within(preview).getByText("Urgent").closest("tr")?.textContent).toContain("(empty)");
    expect(within(preview).getAllByText("12")).toHaveLength(2);
    // Nothing changed yet.
    expect((await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === note)?.type).toBe("text");
    fireEvent.change(within(dialog).getByLabelText("Type"), { target: { value: "date" } });
    fireEvent.change(within(dialog).getByLabelText("Type"), { target: { value: "relation" } });
    expect(within(dialog).getByRole("status").textContent).toContain("every cell will look empty");
    fireEvent.change(within(dialog).getByLabelText("Type"), { target: { value: "select" } });
    expect(within(preview).getAllByText("Urgent")[0]?.closest("tr")?.textContent).toContain("Urgent");
    await ui.user.click(within(dialog).getByRole("button", { name: "Change type" }));
    await waitFor(async () => expect((await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === note)?.type).toBe("select"));
    const options = (await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === note)?.options.map((o) => o.name);
    expect(options).toEqual(["Urgent", "12"]);
    await waitFor(() => expect(within(cellAt(0, columnOf(sample, sample.link) + 1)).getByText("Urgent")).toBeTruthy());
    await ui.memory.api.dbPropertyChangeType({ id: sample.db, propertyId: note, type: "text" });
    expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[0]?.values[note]).toEqual({ type: "text", value: "Urgent" });
  });

  it("renames, recolours, reorders, adds and deletes select options", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await ui.user.click(screen.getByRole("button", { name: "Status column menu" }));
    await ui.user.click(await screen.findByRole("menuitem", { name: "Edit options…" }));
    const dialog = await screen.findByRole("dialog", { name: /Options of “Status”/ });
    const name = within(dialog).getByLabelText("Name of option Open");
    await ui.user.clear(name);
    await ui.user.type(name, "Started");
    fireEvent.change(name, { target: { value: "Started" } });
    await waitFor(async () => expect((await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === sample.status)?.options[0]?.name).toBe("Started"));
    fireEvent.change(within(dialog).getByLabelText("Colour of option Started"), { target: { value: "purple" } });
    await waitFor(async () => expect((await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === sample.status)?.options[0]?.color).toBe("purple"));
    await ui.user.click(within(dialog).getByRole("button", { name: "Move Started down" }));
    await waitFor(async () => expect((await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === sample.status)?.options.map((o) => o.name)).toEqual(["Done", "Started"]));
    await ui.user.type(within(dialog).getByLabelText("New option name"), "Later{Enter}");
    await waitFor(async () => expect((await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === sample.status)?.options.map((o) => o.name)).toEqual(["Done", "Started", "Later"]));
    await ui.user.click(within(dialog).getByRole("button", { name: "Delete option Done" }));
    await waitFor(async () => expect((await ui.memory.api.dbSchema({ id: sample.db })).properties.find((p) => p.id === sample.status)?.options.map((o) => o.name)).toEqual(["Started", "Later"]));
    await ui.user.click(within(dialog).getByRole("button", { name: "Done" }));
    await waitFor(() => expect(within(cellAt(0, columnOf(sample, sample.status))).getByText("Started")).toBeTruthy());
    expect(within(cellAt(1, columnOf(sample, sample.status))).queryByText("Done")).toBeNull();
  });

  it("links rows of another database with a relation picker that searches", async () => {
    const { ui, sample } = await renderDatabase({
      setup: async (memory) => {
        const projects = await memory.api.dbCreate({ parentId: null, title: "Projects" });
        for (const title of ["Launch", "Garden", "Lantern"]) await memory.api.dbRowCreate({ id: projects, title });
      },
    });
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await ui.user.click(screen.getByRole("button", { name: "Add a property" }));
    const dialog = await screen.findByRole("dialog", { name: "Add a property" });
    await ui.user.type(within(dialog).getByLabelText("Property name"), "Project");
    fireEvent.change(within(dialog).getByLabelText("Type"), { target: { value: "relation" } });
    const target = within(dialog).getByLabelText("Link to rows of") as HTMLSelectElement;
    expect(within(target).getAllByRole("option").map((o) => o.textContent)).toEqual(["Tasks", "Projects"]);
    fireEvent.change(target, { target: { value: (await ui.memory.api.treeSnapshot()).find((n) => n.title === "Projects")?.id } });
    await ui.user.type(within(dialog).getByLabelText("Name in the other database (optional)"), "Tasks");
    await ui.user.click(within(dialog).getByRole("button", { name: "Add a property" }));
    const header = await screen.findByRole("columnheader", { name: /Project/ });
    expect(header).toBeTruthy();
    const col = (await ui.memory.api.dbSchema({ id: sample.db })).properties.length;

    await ui.user.dblClick(cellAt(0, col));
    const picker = await screen.findByRole("dialog", { name: "Choose linked rows" });
    await waitFor(() => expect(within(picker).getAllByRole("option")).toHaveLength(3));
    await ui.user.type(within(picker).getByRole("combobox"), "lan");
    await waitFor(() => expect(within(picker).getAllByRole("option").map((o) => o.textContent)).toEqual(["Lantern"]));
    await ui.user.keyboard("{Enter}");
    await waitFor(() => expect(within(cellAt(0, col)).getByText("Lantern")).toBeTruthy());
    const projects = (await ui.memory.api.treeSnapshot()).find((n) => n.title === "Projects")?.id as string;
    const back = (await ui.memory.api.dbSchema({ id: projects })).properties[0]?.id as string;
    expect((await ui.memory.api.dbQuery({ id: projects })).rows.find((r) => r.title === "Lantern")?.values[back]).toEqual({ type: "relation", value: [sample.rows[0]] });
    // Unlink again.
    await ui.user.click(within(picker).getByRole("button", { name: "Unlink Lantern" }));
    await waitFor(() => expect(within(cellAt(0, col)).queryByText("Lantern")).toBeNull());
  });
});

describe("board view", () => {
  it("shows columns from a select property and moves cards by dragging and by keyboard", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await addView(ui, "Board");
    const boardColumns = () => [...document.querySelectorAll<HTMLElement>(".board-column")];
    const columns = await waitFor(() => {
      const found = boardColumns();
      if (found.length < 3) throw new Error("no columns yet");
      return found;
    });
    expect(columns.map((c) => c.getAttribute("aria-label"))).toEqual(["Open, 1 cards", "Done, 1 cards", "No value, 1 cards"]);
    const card = (title: string) => screen.getByText(title).closest(".card") as HTMLElement;
    const column = (name: string) => boardColumns().find((c) => c.getAttribute("aria-label")?.startsWith(name)) as HTMLElement;

    fireEvent.dragStart(card("Plan trip"));
    fireEvent.dragOver(column("Open"));
    fireEvent.drop(column("Open"));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows.find((r) => r.title === "Plan trip")?.values[sample.status]).toEqual({ type: "select", value: sample.open }));
    await waitFor(() => expect(within(column("Open")).getAllByText(/Plan trip|Write report/)).toHaveLength(2));

    // Drop before another card within a column.
    fireEvent.dragStart(card("Plan trip"));
    fireEvent.dragOver(card("Write report"));
    fireEvent.drop(card("Write report"));
    await waitFor(() => expect([...column("Open").querySelectorAll(".card-title")].map((el) => el.textContent)).toEqual(["Plan trip", "Write report"]));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows.map((r) => r.title)).toEqual(["Plan trip", "Write report", "Ship release"]));

    // Alt+arrow moves a card to the next column.
    fireEvent.keyDown(card("Plan trip"), { key: "ArrowRight", altKey: true });
    await waitFor(() => expect([...column("Done").querySelectorAll(".card-title")].map((el) => el.textContent)).toContain("Plan trip"));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows.find((r) => r.title === "Plan trip")?.values[sample.status]).toEqual({ type: "select", value: sample.done }));

    // Add a card in a column.
    await ui.user.click(within(column("Done")).getByRole("button", { name: /New card/ }));
    await ui.user.type(within(column("Done")).getByLabelText(/Title of the new card/), "Brand new{Enter}");
    await waitFor(async () => {
      const row = (await ui.memory.api.dbQuery({ id: sample.db })).rows.find((r) => r.title === "Brand new");
      expect(row?.values[sample.status]).toEqual({ type: "select", value: sample.done });
    });
    await waitFor(() => expect(within(column("Done")).getByText("Brand new")).toBeTruthy());

    // Open a card as a page with Enter.
    card("Brand new").focus();
    await ui.user.keyboard("{Enter}");
    await waitFor(() => expect(ui.app.tabs.activeDatabaseId()).toBe(sample.db));
  });

  it("hides empty groups on request and asks for a group property when the view has none", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await addView(ui, "Board");
    const count = () => document.querySelectorAll(".board-column").length;
    await waitFor(() => expect(count()).toBe(3));
    await ui.user.click(screen.getByRole("button", { name: "Group" }));
    const panel = await screen.findByRole("dialog", { name: "Group" });
    await ui.user.click(within(panel).getByRole("checkbox", { name: "Hide empty groups" }));
    await ui.memory.api.treeTrash({ id: sample.rows[2] as string });
    await ui.app.tree.refresh();
    await waitFor(() => expect(count()).toBe(2));
    fireEvent.change(within(panel).getByLabelText("Group by"), { target: { value: "" } });
    await waitFor(() => expect(screen.getByText("Choose what the columns are")).toBeTruthy());
  });
});

describe("list and calendar views", () => {
  it("shows compact rows and opens one", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await addView(ui, "List");
    const list = await screen.findByRole("list", { name: "Rows of the database" });
    expect(within(list).getAllByRole("button").map((b) => b.querySelector(".card-title")?.textContent)).toEqual(["Write report", "Ship release", "Plan trip"]);
    await ui.user.click(within(list).getByRole("button", { name: /Write report/ }));
    await waitFor(() => expect(ui.app.tabs.activePageId()).toBe(sample.rows[0]));
  });

  it("places rows on a month grid, moves one to another day by dragging and creates a row on a day", async () => {
    const today = new Date();
    const dayOfMonth = (day: number) => dayText(new Date(Date.UTC(today.getFullYear(), today.getMonth(), day)));
    const { ui, sample } = await renderDatabase({
      setup: async (memory) => {
        const [first] = (await memory.api.treeSnapshot()).filter((n) => n.kind === "database");
        const db = first?.id as string;
        const due = (await memory.api.dbSchema({ id: db })).properties.find((p) => p.name === "Due")?.id as string;
        const rows = (await memory.api.dbQuery({ id: db })).rows;
        await memory.api.dbCellSet({ id: db, rowId: rows[0]?.id as string, propertyId: due, value: { type: "date", value: { start: `${dayOfMonth(10)}T09:30` } } });
        await memory.api.dbCellSet({ id: db, rowId: rows[1]?.id as string, propertyId: due, value: { type: "date", value: { start: dayOfMonth(12), end: dayOfMonth(13) } } });
      },
    });
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await addView(ui, "Calendar");
    const day = (n: number) => document.querySelector(`[data-day="${dayOfMonth(n)}"]`) as HTMLElement;
    await waitFor(() => expect(within(day(10)).getByText("Write report")).toBeTruthy());
    expect(within(day(12)).getByText("Ship release")).toBeTruthy();
    expect(within(day(13)).getByText("Ship release")).toBeTruthy();
    expect(day(11).querySelector(".cal-row")).toBeNull();

    fireEvent.dragStart(within(day(10)).getByText("Write report"));
    fireEvent.dragOver(day(20));
    fireEvent.drop(day(20));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[0]?.values[sample.due]).toEqual({ type: "date", value: { start: `${dayOfMonth(20)}T09:30` } }));
    await waitFor(() => expect(within(day(20)).getByText("Write report")).toBeTruthy());

    await ui.user.click(within(day(25)).getByRole("button", { name: `Add a row on ${dayOfMonth(25)}` }));
    await ui.user.type(within(day(25)).getByLabelText(/Title of the new row/), "Party{Enter}");
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows.find((r) => r.title === "Party")?.values[sample.due]).toEqual({ type: "date", value: { start: dayOfMonth(25) } }));

    // Month navigation re-queries the visible range.
    await ui.user.click(screen.getByRole("button", { name: "Next month" }));
    await waitFor(() => expect(document.querySelector(".cal-row")).toBeNull());
    await ui.user.click(screen.getByRole("button", { name: "Today" }));
    await waitFor(() => expect(within(day(20)).getByText("Write report")).toBeTruthy());
  });

  it("asks which date property the calendar uses when it has none", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toHaveLength(3));
    await ui.memory.api.dbPropertyDelete({ id: sample.db, propertyId: sample.due });
    await addView(ui, "Calendar");
    await waitFor(() => expect(screen.getByText("Choose a date property")).toBeTruthy());
  });
});
