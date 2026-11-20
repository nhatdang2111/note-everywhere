import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import { cellAt, columnOf, dbQueryCount, renderDatabase, seedTasks, waitForTable } from "./database-harness.tsx";
import { tick } from "../../test/app-harness.tsx";

const rowTitles = () => [...document.querySelectorAll('[role="gridcell"][data-col="0"] .cell-text')].map((el) => el.textContent);

describe("database table", () => {
  it("opens a database like a page with its title, view tabs and rows, and keeps rows out of the sidebar", async () => {
    const { ui } = await renderDatabase();
    await waitForTable();
    expect((screen.getByLabelText("Database title") as HTMLInputElement).value).toBe("Tasks");
    expect(screen.getByRole("tab", { name: /Table/ }).getAttribute("aria-selected")).toBe("true");
    await waitFor(() => expect(rowTitles()).toEqual(["Write report", "Ship release", "Plan trip"]));
    const tree = within(screen.getByRole("tree"));
    expect(tree.getByText("Tasks")).toBeTruthy();
    expect(tree.queryByText("Write report")).toBeNull();
    expect(screen.getByText("3 rows")).toBeTruthy();
    expect(ui.app.tree.index().byId.get(ui.app.tabs.activePageId() as string)?.kind).toBe("database");
  });

  it("draws only the rows near the viewport of 10,000 rows and fetches pages as it scrolls", async () => {
    const { ui } = await renderDatabase({
      seed: async (memory) => {
        const sample = await seedTasks(memory);
        await memory.control.seedRows(sample.db, 10_000, (at) => ({ title: `Bulk ${at}` }));
        return sample;
      },
    });
    const grid = await waitForTable();
    expect(screen.getByText("10003 rows")).toBeTruthy();
    const drawn = () => grid.querySelectorAll('.db-table-body [role="row"]').length;
    await waitFor(() => expect(rowTitles()).toContain("Write report"));
    expect(drawn()).toBeLessThan(60);
    const queries = dbQueryCount(ui);
    const scroller = document.querySelector(".db-table-scroll") as HTMLElement;
    scroller.scrollTop = 36 * 5000;
    fireEvent.scroll(scroller);
    await waitFor(() => expect(rowTitles()).toContain("Bulk 4999"));
    expect(drawn()).toBeLessThan(60);
    expect(rowTitles()).not.toContain("Write report");
    expect(queries()).toBeLessThanOrEqual(3);
    expect((grid.querySelector(".db-table-body") as HTMLElement).style.height).toBe(`${10_003 * 36}px`);
  }, 30_000);

  it("edits a title and a text-like cell inline: double-click, type, Enter", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    await ui.user.dblClick(cellAt(2, 0));
    const input = within(cellAt(2, 0)).getByRole("textbox");
    await ui.user.clear(input);
    await ui.user.type(input, "Plan the trip{Enter}");
    await waitFor(() => expect(rowTitles()[2]).toBe("Plan the trip"));
    expect((await ui.memory.api.treeSnapshot()).length).toBe(1);
    expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.title).toBe("Plan the trip");

    await ui.user.dblClick(cellAt(0, columnOf(sample, sample.points)));
    const points = within(cellAt(0, columnOf(sample, sample.points))).getByRole("textbox");
    await ui.user.clear(points);
    await ui.user.type(points, "8.5{Enter}");
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[0]?.values[sample.points]).toEqual({ type: "number", value: 8.5 }));

    await ui.user.dblClick(cellAt(1, columnOf(sample, sample.link)));
    await ui.user.type(within(cellAt(1, columnOf(sample, sample.link))).getByRole("textbox"), "https://example.org{Enter}");
    await waitFor(() => expect(within(cellAt(1, columnOf(sample, sample.link))).getByRole("link").getAttribute("href")).toBe("https://example.org"));
  });

  it("keeps editing a number that does not parse and cancels with Escape", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    const col = columnOf(sample, sample.points);
    await ui.user.dblClick(cellAt(2, col));
    const input = within(cellAt(2, col)).getByRole("textbox");
    await ui.user.type(input, "abc{Enter}");
    expect(input.getAttribute("aria-invalid")).toBe("true");
    expect(within(cellAt(2, col)).queryByRole("textbox")).not.toBeNull();
    await ui.user.keyboard("{Escape}");
    await waitFor(() => expect(within(cellAt(2, col)).queryByRole("textbox")).toBeNull());
    expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.points]).toBeUndefined();
  });

  it("chooses a select option, creating one by typing, and clears it", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    const col = columnOf(sample, sample.status);
    await ui.user.dblClick(cellAt(2, col));
    const search = await screen.findByRole("combobox");
    expect(within(screen.getByRole("listbox")).getAllByRole("option").map((o) => o.textContent)).toEqual(["Open", "Done"]);
    await ui.user.click(within(screen.getByRole("listbox")).getByText("Done"));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.status]).toEqual({ type: "select", value: sample.done }));
    expect(screen.queryByRole("combobox")).toBeNull();

    await ui.user.dblClick(cellAt(0, col));
    await ui.user.type(await screen.findByRole("combobox"), "Blocked");
    expect(screen.getByRole("option", { name: /Create “Blocked”/ })).toBeTruthy();
    await ui.user.keyboard("{Enter}");
    await waitFor(async () => {
      const schema = await ui.memory.api.dbSchema({ id: sample.db });
      expect(schema.properties.find((p) => p.id === sample.status)?.options.map((o) => o.name)).toEqual(["Open", "Done", "Blocked"]);
    });
    await waitFor(() => expect(within(cellAt(0, col)).getByText("Blocked")).toBeTruthy());
    expect(search).toBeTruthy();
  });

  it("ticks several multi-select options and edits a date with an end and a time", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    const tagsCol = columnOf(sample, sample.tags);
    await ui.user.dblClick(cellAt(0, tagsCol));
    await ui.user.click(within(await screen.findByRole("listbox")).getByText("Urgent"));
    await waitFor(() => expect(within(cellAt(0, tagsCol)).getByText("Urgent")).toBeTruthy());
    expect(screen.getByRole("listbox").getAttribute("aria-multiselectable")).toBe("true");
    await ui.user.keyboard("{Escape}");
    await waitFor(() => expect(screen.queryByRole("combobox")).toBeNull());

    const dueCol = columnOf(sample, sample.due);
    await ui.user.dblClick(cellAt(2, dueCol));
    const dialog = await screen.findByRole("dialog", { name: "Edit date" });
    fireEvent.change(within(dialog).getByLabelText("Date"), { target: { value: "2026-12-01" } });
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.due]).toEqual({ type: "date", value: { start: "2026-12-01" } }));
    await ui.user.click(within(dialog).getByLabelText("End date", { selector: "input[type=checkbox]" }));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.due]).toEqual({ type: "date", value: { start: "2026-12-01", end: "2026-12-01" } }));
    await ui.user.click(within(dialog).getByLabelText("Include time"));
    await waitFor(async () => {
      const value = (await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.due];
      expect(value).toEqual({ type: "date", value: { start: "2026-12-01T09:00", end: "2026-12-01T10:00" } });
    });
    await ui.user.click(within(dialog).getByRole("button", { name: "Clear" }));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.due]).toBeUndefined());
  });

  it("toggles a checkbox cell", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    await ui.user.click(within(cellAt(1, columnOf(sample, sample.flag))).getByRole("checkbox"));
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[1]?.values[sample.flag]).toEqual({ type: "checkbox", value: true }));
  });

  it("navigates and edits by keyboard: arrows, Enter, typing, Delete, Escape", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    const points = columnOf(sample, sample.points);
    cellAt(0, 0).focus();
    await ui.user.keyboard("{ArrowDown}{ArrowDown}");
    expect(cellAt(2, 0).getAttribute("aria-selected")).toBe("true");
    await ui.user.keyboard("{ArrowUp}{ArrowRight}{ArrowRight}{ArrowRight}");
    expect(cellAt(1, 3).getAttribute("aria-selected")).toBe("true");
    cellAt(0, points).focus();
    await ui.user.keyboard("{Delete}");
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[0]?.values[sample.points]).toBeUndefined());
    await ui.user.keyboard("7{Enter}");
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[0]?.values[sample.points]).toEqual({ type: "number", value: 7 }));
    // Enter moved the selection one row down.
    await waitFor(() => expect(cellAt(1, points).getAttribute("aria-selected")).toBe("true"));
    await ui.user.keyboard("{Enter}");
    await ui.user.keyboard("99{Escape}");
    await tick(20);
    expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[1]?.values[sample.points]).toBeUndefined();
    cellAt(1, points).focus();
    await ui.user.keyboard("{Control>}{ArrowDown}{/Control}");
    expect(cellAt(2, points).getAttribute("aria-selected")).toBe("true");
    cellAt(0, 0).focus();
    await ui.user.keyboard("{Control>}{Enter}{/Control}");
    await waitFor(() => expect(ui.app.tabs.activePageId()).toBe(sample.rows[0]));
  });

  it("copies and pastes cell values, as exact values between cells of one type and as text otherwise", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    const grid = await waitForTable();
    const store: Record<string, string> = {};
    const data = { setData: (type: string, value: string) => void (store[type] = value), getData: (type: string) => store[type] ?? "" };
    const statusCol = columnOf(sample, sample.status);
    cellAt(0, statusCol).focus();
    fireEvent.mouseDown(cellAt(0, statusCol));
    fireEvent.copy(grid, { clipboardData: data });
    expect(store["text/plain"]).toBe("Open");
    cellAt(2, statusCol).focus();
    fireEvent.mouseDown(cellAt(2, statusCol));
    fireEvent.paste(grid, { clipboardData: data });
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.status]).toEqual({ type: "select", value: sample.open }));
    // Text into a number cell, and a name that matches no option creates it.
    const pointsCol = columnOf(sample, sample.points);
    fireEvent.mouseDown(cellAt(1, pointsCol));
    fireEvent.paste(grid, { clipboardData: { getData: (type: string) => (type === "text/plain" ? " 42 " : "") } });
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[1]?.values[sample.points]).toEqual({ type: "number", value: 42 }));
    fireEvent.mouseDown(cellAt(0, pointsCol));
    fireEvent.paste(grid, { clipboardData: { getData: (type: string) => (type === "text/plain" ? "lots" : "") } });
    await screen.findByText("That text does not fit “Points”.");
    fireEvent.mouseDown(cellAt(1, statusCol));
    fireEvent.paste(grid, { clipboardData: { getData: (type: string) => (type === "text/plain" ? "Stuck" : "") } });
    await waitFor(async () => {
      const schema = await ui.memory.api.dbSchema({ id: sample.db });
      expect(schema.properties.find((p) => p.id === sample.status)?.options.map((o) => o.name)).toContain("Stuck");
    });
    fireEvent.mouseDown(cellAt(0, 0));
    fireEvent.paste(grid, { clipboardData: { getData: (type: string) => (type === "text/plain" ? "Renamed by paste" : "") } });
    await waitFor(() => expect(rowTitles()[0]).toBe("Renamed by paste"));
  });

  it("keeps an in-progress cell edit when the database changes underneath it", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    const pointsCol = columnOf(sample, sample.points);
    await ui.user.dblClick(cellAt(2, pointsCol));
    const input = within(cellAt(2, pointsCol)).getByRole("textbox") as HTMLInputElement;
    await ui.user.type(input, "12");
    // Another device edits a different cell and renames a row; this device also saves a cell.
    await ui.memory.control.receiveRemoteCell(sample.db, sample.rows[0] as string, sample.points, { type: "number", value: 99 });
    await ui.memory.control.receiveRemoteCell(sample.db, sample.rows[1] as string, "title", { type: "text", value: "Ship it" });
    await waitFor(() => expect(within(cellAt(0, pointsCol)).getByText("99")).toBeTruthy());
    await waitFor(() => expect(rowTitles()[1]).toBe("Ship it"));
    const still = within(cellAt(2, pointsCol)).getByRole("textbox") as HTMLInputElement;
    expect(still.value).toBe("12");
    expect(document.activeElement).toBe(still);
    await ui.user.type(still, "3{Enter}");
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[2]?.values[sample.points]).toEqual({ type: "number", value: 123 }));
  });

  it("reacts to database:changed events from this device's own edits made elsewhere", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Plan trip"));
    await ui.memory.api.dbRowCreate({ id: sample.db, title: "Added elsewhere" });
    await waitFor(() => expect(rowTitles()).toContain("Added elsewhere"));
    expect(screen.getByText("4 rows")).toBeTruthy();
  });

  it("opens a row as a page with its properties above the content and edits them there", async () => {
    const { ui, sample } = await renderDatabase();
    await waitFor(() => expect(rowTitles()).toContain("Write report"));
    await ui.user.click(screen.getByRole("button", { name: "Open Write report" }));
    await waitFor(() => expect(ui.app.tabs.activePageId()).toBe(sample.rows[0]));
    const properties = await screen.findByRole("region", { name: "Properties" });
    expect(within(properties).getByText("Status")).toBeTruthy();
    await waitFor(() => expect(within(properties).getByText("Open")).toBeTruthy());
    expect((screen.getByLabelText("Page title") as HTMLInputElement).value).toBe("Write report");
    expect(screen.getByRole("tab", { selected: true }).textContent).toContain("Write report");
    // Edit the number there.
    await ui.user.click(within(properties).getByText("5"));
    const input = within(properties).getByRole("textbox");
    await ui.user.clear(input);
    await ui.user.type(input, "13{Enter}");
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[0]?.values[sample.points]).toEqual({ type: "number", value: 13 }));
    // Rename it from the page.
    const title = screen.getByLabelText("Page title");
    await ui.user.clear(title);
    await ui.user.type(title, "Write the report");
    await ui.user.tab();
    await waitFor(async () => expect((await ui.memory.api.dbQuery({ id: sample.db })).rows[0]?.title).toBe("Write the report"));
    // The breadcrumb leads back to the database.
    await ui.user.click(within(screen.getByRole("navigation", { name: "Location" })).getByRole("button", { name: "Tasks" }));
    await waitFor(() => expect(ui.app.tabs.activePageId()).toBe(sample.db));
    await waitFor(() => expect(rowTitles()[0]).toBe("Write the report"));
  });
});
