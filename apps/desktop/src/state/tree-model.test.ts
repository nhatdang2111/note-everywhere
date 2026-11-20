import { describe, expect, it } from "vitest";
import type { TreeNode } from "../api/contract.ts";
import { buildIndex, isDescendant, planDrop, planStep, visibleRows, zoneFor } from "./tree-model.ts";

const node = (id: string, parentId: string | null, kind: TreeNode["kind"] = "page"): TreeNode => ({
  id,
  parentId,
  kind,
  title: id,
  icon: null,
  favorite: false,
  createdAt: 0,
  updatedAt: 0,
});

// root: A (B (C), D), E
const snapshot = [node("A", null, "folder"), node("B", "A"), node("C", "B"), node("D", "A"), node("E", null)];

describe("tree model", () => {
  const index = buildIndex(snapshot);

  it("lists children in snapshot order", () => {
    expect(index.children.get(null)).toEqual(["A", "E"]);
    expect(index.children.get("A")).toEqual(["B", "D"]);
  });

  it("shows only expanded branches, at any depth", () => {
    expect(visibleRows(index, new Set()).map((row) => row.node.id)).toEqual(["A", "E"]);
    const rows = visibleRows(index, new Set(["A", "B"]));
    expect(rows.map((row) => [row.node.id, row.depth])).toEqual([
      ["A", 0],
      ["B", 1],
      ["C", 2],
      ["D", 1],
      ["E", 0],
    ]);
    expect(rows[0]?.expanded).toBe(true);
    expect(rows[1]?.hasChildren).toBe(true);
  });

  it("does not treat a collapsed leaf as expanded", () => {
    expect(visibleRows(index, new Set(["E"]))[1]?.expanded).toBe(false);
  });

  it("finds descendants", () => {
    expect(isDescendant(index, "C", "A")).toBe(true);
    expect(isDescendant(index, "A", "C")).toBe(false);
    expect(isDescendant(index, "A", "A")).toBe(false);
  });

  describe("planDrop", () => {
    it("re-parents on an inside drop and appends", () => {
      expect(planDrop(index, "E", "A", "inside")).toEqual({ id: "E", parentId: "A" });
    });

    it("reorders before and after a sibling", () => {
      expect(planDrop(index, "D", "B", "before")).toEqual({ id: "D", parentId: "A", index: 0 });
      expect(planDrop(index, "E", "B", "after")).toEqual({ id: "E", parentId: "A", index: 1 });
    });

    it("counts the destination index with the dragged node already removed", () => {
      // A's children are [B, D]; moving B after D leaves [D] then inserts at 1.
      expect(planDrop(index, "B", "D", "after")).toEqual({ id: "B", parentId: "A", index: 1 });
    });

    it("refuses to drop a node into itself or its own descendants", () => {
      expect(planDrop(index, "A", "A", "inside")).toBeNull();
      expect(planDrop(index, "A", "B", "inside")).toBeNull();
      expect(planDrop(index, "A", "C", "before")).toBeNull();
      expect(planDrop(index, "B", "C", "after")).toBeNull();
    });

    it("returns null when nothing would change", () => {
      expect(planDrop(index, "B", "D", "before")).toBeNull();
      expect(planDrop(index, "B", "A", "inside")).toEqual({ id: "B", parentId: "A" });
      expect(planDrop(index, "D", "A", "inside")).toBeNull();
    });

    it("moves to the top level next to a root node", () => {
      expect(planDrop(index, "C", "E", "before")).toEqual({ id: "C", parentId: null, index: 1 });
    });

    it("ignores unknown ids", () => {
      expect(planDrop(index, "nope", "A", "inside")).toBeNull();
    });
  });

  describe("planStep", () => {
    it("moves among siblings", () => {
      expect(planStep(index, "D", "up")).toEqual({ id: "D", parentId: "A", index: 0 });
      expect(planStep(index, "B", "down")).toEqual({ id: "B", parentId: "A", index: 1 });
      expect(planStep(index, "B", "up")).toBeNull();
      expect(planStep(index, "D", "down")).toBeNull();
    });

    it("moves out one level, right after the old parent", () => {
      expect(planStep(index, "C", "out")).toEqual({ id: "C", parentId: "A", index: 1 });
      expect(planStep(index, "B", "out")).toEqual({ id: "B", parentId: null, index: 1 });
      expect(planStep(index, "E", "out")).toBeNull();
    });
  });

  it("picks the drop zone from the pointer position", () => {
    expect(zoneFor(2, 28)).toBe("before");
    expect(zoneFor(14, 28)).toBe("inside");
    expect(zoneFor(26, 28)).toBe("after");
  });
});
