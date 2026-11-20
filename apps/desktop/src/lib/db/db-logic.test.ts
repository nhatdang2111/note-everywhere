import { describe, expect, it } from "vitest";
import type { DatabaseSchema, DbProperty, FilterNode, QueryBucket, QueryRow, ViewDef } from "../../api/contract.ts";
import { applyCardMove, planCardMove, stepCard } from "./board-model.ts";
import { monthGrid, moveDate, placeRows, planDayDrop, shiftMonth } from "./calendar-model.ts";
import { cellToText, conversionKind, parseCellText, readCell } from "./cell-convert.ts";
import { vietnameseCompare } from "./collation.ts";
import { MAX_GROUP_DEPTH, emptyGroup, fromFilter, groupDepth, newCondition, toFilter, updateNode, withProperty } from "./filter-model.ts";
import { OPERATORS } from "./operators.ts";
import { moveCell } from "./table-nav.ts";
import { orderedColumns, withColumnMoved, withColumnVisible, withColumnWidth } from "./view-model.ts";
import { computeWindow, pagesFor, scrollToReveal } from "./virtual-window.ts";

const select = (id: string, ...names: string[]): DbProperty => ({
  id,
  name: id,
  type: "select",
  options: names.map((name) => ({ id: `o-${name.toLowerCase()}`, name, color: "gray" })),
});
const prop = (id: string, type: DbProperty["type"], options: DbProperty["options"] = []): DbProperty => ({ id, name: id, type, options });

describe("table virtualisation window", () => {
  it("draws only the rows near the viewport of a 10,000 row list, and the scrollbar keeps the full height", () => {
    const win = computeWindow({ scrollTop: 0, viewportHeight: 600, rowHeight: 36, total: 10_000 });
    expect(win.start).toBe(0);
    expect(win.end).toBeLessThan(40);
    expect(win.totalHeight).toBe(360_000);
    const middle = computeWindow({ scrollTop: 180_000, viewportHeight: 600, rowHeight: 36, total: 10_000 });
    expect(middle.start).toBe(5000 - 6);
    expect(middle.end - middle.start).toBeLessThan(40);
    expect(middle.offsetTop).toBe(middle.start * 36);
  });

  it("clamps at both ends and handles an empty list", () => {
    expect(computeWindow({ scrollTop: 9_999_999, viewportHeight: 600, rowHeight: 36, total: 100 }).end).toBe(100);
    expect(computeWindow({ scrollTop: -50, viewportHeight: 600, rowHeight: 36, total: 100 }).start).toBe(0);
    expect(computeWindow({ scrollTop: 0, viewportHeight: 600, rowHeight: 36, total: 0 })).toEqual({ start: 0, end: 0, offsetTop: 0, totalHeight: 0 });
  });

  it("asks only for the query pages that cover the window", () => {
    expect(pagesFor(0, 30, 200)).toEqual([0]);
    expect(pagesFor(190, 215, 200)).toEqual([0, 1]);
    expect(pagesFor(5000, 5030, 200)).toEqual([25]);
    expect(pagesFor(10, 10, 200)).toEqual([]);
  });

  it("scrolls just enough to reveal a row", () => {
    expect(scrollToReveal(5, 36, 0, 600)).toBe(0);
    expect(scrollToReveal(30, 36, 0, 600)).toBe(30 * 36 + 36 - 600);
    expect(scrollToReveal(1, 36, 400, 600)).toBe(36);
  });
});

describe("keyboard cell navigation", () => {
  const size = { rows: 100, cols: 4, page: 10 };
  it("moves by cell, by page and to the edges, staying inside the grid", () => {
    expect(moveCell({ row: 0, col: 0 }, "ArrowUp", size)).toEqual({ row: 0, col: 0 });
    expect(moveCell({ row: 5, col: 1 }, "ArrowDown", size)).toEqual({ row: 6, col: 1 });
    expect(moveCell({ row: 5, col: 3 }, "ArrowRight", size)).toEqual({ row: 5, col: 3 });
    expect(moveCell({ row: 5, col: 1 }, "PageDown", size)).toEqual({ row: 15, col: 1 });
    expect(moveCell({ row: 5, col: 1 }, "ArrowDown", size, true)).toEqual({ row: 99, col: 1 });
    expect(moveCell({ row: 5, col: 1 }, "Home", size)).toEqual({ row: 5, col: 0 });
    expect(moveCell({ row: 5, col: 1 }, "a", size)).toBeNull();
  });
});

describe("filter builder serialisation", () => {
  const props = [prop("n", "number"), select("s", "Open", "Done"), prop("d", "date"), prop("c", "checkbox")];

  it("builds the contract filter from the editable tree and drops what is not ready", () => {
    const root = emptyGroup("or");
    const title = { ...newCondition(props, "title"), operator: "contains" as const, value: "plan" };
    const number = { ...newCondition(props, "n"), operator: "gte" as const, value: "3" };
    const unfinished = { ...newCondition(props, "n"), operator: "lt" as const, value: "" };
    const empty = { ...newCondition(props, "s"), operator: "is_empty" as const };
    root.children = [title, number, unfinished, empty, { ...emptyGroup(), children: [] }];
    expect(toFilter(root, props)).toEqual({
      type: "group",
      logic: "or",
      children: [
        { type: "condition", property: "title", operator: "contains", value: "plan" },
        { type: "condition", property: "n", operator: "gte", value: 3 },
        { type: "condition", property: "s", operator: "is_empty" },
      ],
    });
    expect(toFilter(emptyGroup(), props)).toBeNull();
  });

  it("rebuilds the editable tree from a saved filter and serialises back to the same filter", () => {
    const filter: FilterNode = {
      type: "group",
      logic: "and",
      children: [
        { type: "condition", property: "s", operator: "is", value: "o-open" },
        {
          type: "group",
          logic: "or",
          children: [
            { type: "condition", property: "n", operator: "gt", value: 10 },
            { type: "condition", property: "c", operator: "is_checked" },
          ],
        },
      ],
    };
    const builder = fromFilter(filter);
    expect(groupDepth(builder)).toBe(2);
    expect(toFilter(builder, props)).toEqual(filter);
    expect(fromFilter(null).children).toEqual([]);
  });

  it("offers exactly the operators of each property type and resets an operator that no longer fits", () => {
    expect(OPERATORS.checkbox).toEqual(["is_checked", "is_unchecked"]);
    expect(OPERATORS.date).toContain("on_or_after");
    expect(OPERATORS.number).not.toContain("contains");
    const condition = { ...newCondition(props, "title"), operator: "contains" as const, value: "x" };
    const moved = withProperty(condition, props, "n");
    expect(moved.operator).toBe("is");
    expect(moved.value).toBe("");
    expect(withProperty(newCondition(props, "n"), props, "n").operator).toBe("is");
  });

  it("updates and removes nodes by id and limits nesting to three groups", () => {
    const root = emptyGroup();
    const condition = newCondition(props, "title");
    root.children = [condition];
    const edited = updateNode(root, condition.id, (node) => (node.type === "condition" ? { ...node, value: "z" } : node));
    expect(edited.children[0]).toMatchObject({ value: "z" });
    expect(updateNode(edited, condition.id, () => null).children).toEqual([]);
    expect(MAX_GROUP_DEPTH).toBe(3);
  });
});

describe("board reducer", () => {
  const row = (id: string): QueryRow => ({ id, title: id, icon: null, values: {} });
  const columns = (): QueryBucket[] => [
    { key: "a", count: 3, rows: [row("r1"), row("r2"), row("r3")] },
    { key: "b", count: 1, rows: [row("r4")] },
    { key: null, count: 0, rows: [] },
  ];

  it("plans a move between columns, to the end or before a card, and nothing for a drop that changes nothing", () => {
    expect(planCardMove(columns(), "r1", "a", "b", null)).toEqual({ rowId: "r1", from: "a", group: "b", beforeId: null });
    expect(planCardMove(columns(), "r1", "a", "b", "r4")).toMatchObject({ beforeId: "r4" });
    expect(planCardMove(columns(), "r1", "a", "a", "r2")).toBeNull();
    expect(planCardMove(columns(), "r3", "a", "a", null)).toBeNull();
    expect(planCardMove(columns(), "r1", "a", "a", "r1")).toBeNull();
    expect(planCardMove(columns(), "r1", "a", "a", "r3")).toMatchObject({ beforeId: "r3" });
    expect(planCardMove(columns(), "ghost", "a", "b", null)).toBeNull();
  });

  it("applies a move within and between columns, keeping counts right", () => {
    const within = applyCardMove(columns(), { rowId: "r1", from: "a", group: "a", beforeId: "r3" });
    expect(within[0]?.rows.map((r) => r.id)).toEqual(["r2", "r1", "r3"]);
    const between = applyCardMove(columns(), { rowId: "r2", from: "a", group: "b", beforeId: "r4" });
    expect(between[0]?.rows.map((r) => r.id)).toEqual(["r1", "r3"]);
    expect(between[1]?.rows.map((r) => r.id)).toEqual(["r2", "r4"]);
    expect([between[0]?.count, between[1]?.count]).toEqual([2, 2]);
    const empty = applyCardMove(columns(), { rowId: "r4", from: "b", group: null, beforeId: null });
    expect(empty[2]?.rows.map((r) => r.id)).toEqual(["r4"]);
  });

  it("steps a card with the keyboard", () => {
    expect(stepCard(columns(), "r2", "right")).toMatchObject({ group: "b", beforeId: null });
    expect(stepCard(columns(), "r2", "up")).toMatchObject({ group: "a", beforeId: "r1" });
    expect(stepCard(columns(), "r2", "down")).toMatchObject({ beforeId: null });
    expect(stepCard(columns(), "r1", "left")).toBeNull();
    expect(stepCard(columns(), "r1", "up")).toBeNull();
  });
});

describe("calendar reducer", () => {
  const dated = (id: string, start: string, end?: string): QueryRow => ({ id, title: id, icon: null, values: { d: { type: "date", value: end ? { start, end } : { start } } } });

  it("builds six full weeks starting on Monday", () => {
    const grid = monthGrid(2026, 10);
    expect(grid.days).toHaveLength(42);
    expect(grid.days[0]?.date).toBe("2026-10-26");
    expect(grid.days.find((day) => day.date === "2026-11-01")?.inMonth).toBe(true);
    expect(grid.days[0]?.inMonth).toBe(false);
    expect(monthGrid(2026, 10, 0).days[0]?.date).toBe("2026-11-01");
    expect(shiftMonth(2026, 11, 1)).toEqual({ year: 2027, month: 0 });
  });

  it("puts a row on every day it covers", () => {
    const placed = placeRows(monthGrid(2026, 10), [dated("a", "2026-11-03"), dated("b", "2026-11-05", "2026-11-07")], "d");
    expect(placed.get("2026-11-03")?.map((r) => r.id)).toEqual(["a"]);
    expect(["05", "06", "07"].map((day) => placed.get(`2026-11-${day}`)?.map((r) => r.id))).toEqual([["b"], ["b"], ["b"]]);
    expect(placed.get("2026-11-08")).toEqual([]);
  });

  it("moves a date to another day keeping its length and time, and does nothing for the same day", () => {
    expect(moveDate({ start: "2026-11-05T14:30", end: "2026-11-07T16:00" }, "2026-11-10")).toEqual({ start: "2026-11-10T14:30", end: "2026-11-12T16:00" });
    expect(moveDate({ start: "2026-11-30" }, "2026-12-02")).toEqual({ start: "2026-12-02" });
    const rows = [dated("a", "2026-11-03")];
    expect(planDayDrop(rows, "a", "d", "2026-11-03")).toBeNull();
    expect(planDayDrop(rows, "a", "d", "2026-11-20")).toEqual({ start: "2026-11-20" });
    expect(planDayDrop(rows, "ghost", "d", "2026-11-20")).toBeNull();
  });
});

describe("Vietnamese collation", () => {
  it("orders by the Vietnamese alphabet, then by tone", () => {
    const sorted = ["ê", "e", "đ", "d", "ă", "a", "â", "b", "c"].sort(vietnameseCompare);
    expect(sorted).toEqual(["a", "ă", "â", "b", "c", "d", "đ", "e", "ê"]);
    expect(["ạ", "á", "ã", "ả", "à", "a"].sort(vietnameseCompare)).toEqual(["a", "à", "ả", "ã", "á", "ạ"]);
    expect(["Hà", "Hạ", "Ha", "Hải"].sort(vietnameseCompare)).toEqual(["Ha", "Hà", "Hạ", "Hải"]);
    expect(vietnameseCompare("2", "a")).toBeLessThan(0);
  });
});

describe("cell conversion", () => {
  const status = select("st", "Open", "Done");

  it("follows the contract table and keeps stored values untouched", () => {
    expect(conversionKind("text", "number")).toBe("keeps");
    expect(conversionKind("relation", "text")).toBe("blank");
    expect(conversionKind("date", "number")).toBe("blank");
    expect(conversionKind("select", "select")).toBe("same");
    const asNumber = prop("p", "number");
    expect(readCell(asNumber, { type: "text", value: "12.5" })).toEqual({ type: "number", value: 12.5 });
    expect(readCell(asNumber, { type: "text", value: "abc" })).toBeUndefined();
    expect(readCell(prop("p", "checkbox"), { type: "text", value: "Yes" })).toEqual({ type: "checkbox", value: true });
    expect(readCell(prop("p", "date"), { type: "text", value: "2026-11-21" })).toEqual({ type: "date", value: { start: "2026-11-21" } });
    expect(readCell(prop("p", "date"), { type: "text", value: "21/11/2026" })).toBeUndefined();
    expect(readCell({ ...status, type: "text" }, { type: "select", value: "o-done" })).toEqual({ type: "text", value: "Done" });
    expect(readCell({ ...status, type: "multi_select" }, { type: "select", value: "o-done" })).toEqual({ type: "multi_select", value: ["o-done"] });
    expect(readCell({ ...status, type: "select" }, { type: "multi_select", value: ["o-gone", "o-open"] })).toEqual({ type: "select", value: "o-open" });
    expect(readCell(prop("p", "text"), { type: "date", value: { start: "2026-11-21", end: "2026-11-23" } })).toEqual({ type: "text", value: "2026-11-21 -> 2026-11-23" });
    expect(readCell(prop("p", "number"), { type: "checkbox", value: true })).toEqual({ type: "number", value: 1 });
    expect(readCell(prop("p", "text"), { type: "relation", value: ["x"] })).toBeUndefined();
  });

  it("matches option names ignoring case and accents, and reads a deleted option as empty", () => {
    const tags = { ...select("t", "Cần làm", "Xong"), type: "multi_select" as const };
    expect(readCell(tags, { type: "text", value: "can lam, XONG, other" })).toEqual({ type: "multi_select", value: ["o-cần làm", "o-xong"] });
    expect(readCell(status, { type: "select", value: "o-deleted" })).toBeUndefined();
  });

  it("parses and prints cell text for copy and paste", () => {
    expect(parseCellText(prop("n", "number"), " 7 ")).toEqual({ type: "number", value: 7 });
    expect(parseCellText(status, "done")).toEqual({ type: "select", value: "o-done" });
    expect(parseCellText(status, "nothing")).toBeUndefined();
    expect(cellToText(status, { type: "select", value: "o-open" })).toBe("Open");
    expect(cellToText(prop("r", "relation"), { type: "relation", value: ["a", "b"] }, { a: "Alpha", b: "Beta" })).toBe("Alpha, Beta");
    expect(cellToText(prop("c", "checkbox"), { type: "checkbox", value: true })).toBe("true");
  });
});

describe("view model", () => {
  const schema: DatabaseSchema = { properties: [prop("a", "text"), prop("b", "number"), prop("c", "date")], views: [] };
  const view: ViewDef = { id: "v", name: "T", kind: "table", filter: null, sorts: [], group: null, columns: [{ property: "b", visible: true, width: 200 }, { property: "gone", visible: true }], dateProperty: null };

  it("lists the view's columns first, ignores deleted properties and appends new ones", () => {
    expect(orderedColumns(schema, view).map((c) => c.property.id)).toEqual(["b", "a", "c"]);
    expect(orderedColumns(schema, view)[0]?.width).toBe(200);
  });

  it("changes width within limits, visibility and order", () => {
    expect(orderedColumns(schema, withColumnWidth(schema, view, "a", 5000))[1]?.width).toBe(640);
    expect(orderedColumns(schema, withColumnWidth(schema, view, "a", 10))[1]?.width).toBe(80);
    expect(orderedColumns(schema, withColumnVisible(schema, view, "c", false)).map((c) => c.visible)).toEqual([true, true, false]);
    expect(orderedColumns(schema, withColumnMoved(schema, view, "c", 0)).map((c) => c.property.id)).toEqual(["c", "b", "a"]);
  });
});
