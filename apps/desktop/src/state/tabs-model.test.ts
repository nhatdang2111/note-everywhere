import { describe, expect, it } from "vitest";
import { activate, activeTab, canGo, closeTab, currentTarget, go, initialTabs, openNewEmptyTab, openTarget, pruneMissing, type Target } from "./tabs-model.ts";

const page = (pageId: string): Target => ({ kind: "page", pageId });
const HOME: Target = { kind: "home" };

describe("tabs model", () => {
  it("navigates the active tab and keeps its history", () => {
    let state = initialTabs();
    state = openTarget(state, page("a"));
    state = openTarget(state, page("b"));
    expect(state.tabs).toHaveLength(1);
    expect(currentTarget(activeTab(state))).toEqual(page("b"));
    state = go(state, -1);
    expect(currentTarget(activeTab(state))).toEqual(page("a"));
    expect(canGo(state, 1)).toBe(true);
    state = go(state, 1);
    expect(currentTarget(activeTab(state))).toEqual(page("b"));
    expect(canGo(state, 1)).toBe(false);
  });

  it("drops forward history when navigating after going back", () => {
    let state = openTarget(openTarget(initialTabs(), page("a")), page("b"));
    state = go(state, -1);
    state = openTarget(state, page("c"));
    expect(canGo(state, 1)).toBe(false);
    expect(activeTab(go(state, -1)).history.map((target) => target.kind)).toContain("page");
  });

  it("activates the tab that already shows a page instead of opening it twice", () => {
    let state = openTarget(initialTabs(), page("a"));
    state = openTarget(state, page("b"), true);
    expect(state.tabs).toHaveLength(2);
    const second = state.activeId;
    state = openTarget(state, page("a"));
    expect(state.tabs).toHaveLength(2);
    expect(state.activeId).not.toBe(second);
    expect(currentTarget(activeTab(state))).toEqual(page("a"));
  });

  it("opens a new empty tab and switches between tabs", () => {
    let state = openNewEmptyTab(initialTabs(page("a")), HOME);
    expect(state.tabs).toHaveLength(2);
    const first = state.tabs[0]?.id as string;
    state = activate(state, first);
    expect(state.activeId).toBe(first);
    expect(activate(state, "missing").activeId).toBe(first);
  });

  it("closes tabs and never leaves zero", () => {
    let state = openNewEmptyTab(initialTabs(page("a")), page("b"));
    state = closeTab(state, state.activeId, HOME);
    expect(state.tabs).toHaveLength(1);
    expect(currentTarget(activeTab(state))).toEqual(page("a"));
    state = closeTab(state, state.activeId, HOME);
    expect(state.tabs).toHaveLength(1);
    expect(currentTarget(activeTab(state))).toEqual(HOME);
  });

  it("prunes history entries for deleted pages", () => {
    let state = openTarget(openTarget(openTarget(initialTabs(), page("a")), page("b")), page("c"));
    state = pruneMissing(state, (id) => id !== "b", HOME);
    const tab = activeTab(state);
    expect(tab.history.filter((target) => target.kind === "page").map((target) => (target as { pageId: string }).pageId)).toEqual(["a", "c"]);
    expect(currentTarget(tab)).toEqual(page("c"));
  });

  it("falls back when the shown page is deleted", () => {
    let state = openTarget(openTarget(initialTabs(), page("a")), page("b"));
    state = pruneMissing(state, (id) => id !== "b", HOME);
    // Lands on the nearest surviving entry, never on the deleted page.
    expect(currentTarget(activeTab(state))).toEqual(page("a"));
    expect(canGo(state, -1)).toBe(true);

    state = pruneMissing(state, () => false, HOME);
    expect(currentTarget(activeTab(state)).kind).toBe("home");
  });
});
