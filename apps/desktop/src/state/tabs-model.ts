import type { NodeId } from "../api/contract.ts";

/** What a tab shows. */
export type Target =
  | { kind: "home" }
  | { kind: "page"; pageId: NodeId }
  | { kind: "trash" }
  | { kind: "vault" }
  | { kind: "settings" };

/** One tab with its own back/forward history. `history[index]` is what it shows. */
export interface Tab {
  id: string;
  history: Target[];
  index: number;
}

export interface TabsState {
  tabs: Tab[];
  activeId: string;
}

export const sameTarget = (a: Target, b: Target): boolean =>
  a.kind === b.kind && (a.kind !== "page" || (b.kind === "page" && a.pageId === b.pageId));

export const currentTarget = (tab: Tab): Target => tab.history[tab.index] as Target;

// Tabs are restored from storage, so ids must not repeat across sessions.
let counter = 0;
const newTabId = () => `tab-${Date.now().toString(36)}-${(counter += 1)}`;

export function initialTabs(first: Target = { kind: "home" }): TabsState {
  const tab: Tab = { id: newTabId(), history: [first], index: 0 };
  return { tabs: [tab], activeId: tab.id };
}

export const activeTab = (state: TabsState): Tab => state.tabs.find((tab) => tab.id === state.activeId) ?? (state.tabs[0] as Tab);

function replaceTab(state: TabsState, id: string, update: (tab: Tab) => Tab): TabsState {
  return { ...state, tabs: state.tabs.map((tab) => (tab.id === id ? update(tab) : tab)) };
}

/**
 * Navigate. A page that is already open in some tab just activates that tab;
 * `newTab` opens it beside the others; otherwise the active tab navigates and
 * keeps its history.
 */
export function openTarget(state: TabsState, target: Target, newTab = false): TabsState {
  const existing = state.tabs.find((tab) => sameTarget(currentTarget(tab), target));
  if (existing) return { ...state, activeId: existing.id };
  if (newTab) {
    const tab: Tab = { id: newTabId(), history: [target], index: 0 };
    return { tabs: [...state.tabs, tab], activeId: tab.id };
  }
  return replaceTab(state, state.activeId, (tab) => {
    if (sameTarget(currentTarget(tab), target)) return tab;
    const history = [...tab.history.slice(0, tab.index + 1), target];
    return { ...tab, history, index: history.length - 1 };
  });
}

export function openNewEmptyTab(state: TabsState, target: Target): TabsState {
  const tab: Tab = { id: newTabId(), history: [target], index: 0 };
  return { tabs: [...state.tabs, tab], activeId: tab.id };
}

export function activate(state: TabsState, id: string): TabsState {
  return state.tabs.some((tab) => tab.id === id) ? { ...state, activeId: id } : state;
}

/** Close a tab; the last tab is replaced by an empty one rather than removed. */
export function closeTab(state: TabsState, id: string, fallback: Target): TabsState {
  const at = state.tabs.findIndex((tab) => tab.id === id);
  if (at < 0) return state;
  const tabs = state.tabs.filter((tab) => tab.id !== id);
  if (tabs.length === 0) return initialTabs(fallback);
  const activeId = state.activeId === id ? (tabs[Math.min(at, tabs.length - 1)] as Tab).id : state.activeId;
  return { tabs, activeId };
}

export const canGo = (state: TabsState, direction: -1 | 1): boolean => {
  const tab = activeTab(state);
  const next = tab.index + direction;
  return next >= 0 && next < tab.history.length;
};

export function go(state: TabsState, direction: -1 | 1): TabsState {
  if (!canGo(state, direction)) return state;
  return replaceTab(state, state.activeId, (tab) => ({ ...tab, index: tab.index + direction }));
}

/**
 * Drop history entries for pages that no longer exist so Back never lands on a
 * dead page. Entries other than pages are always kept.
 */
export function pruneMissing(state: TabsState, exists: (pageId: NodeId) => boolean, fallback: Target): TabsState {
  const tabs = state.tabs.map((tab) => {
    const current = currentTarget(tab);
    const history: Target[] = [];
    let index = 0;
    tab.history.forEach((target, at) => {
      if (target.kind === "page" && !exists(target.pageId)) return;
      if (at === tab.index) index = history.length;
      history.push(target);
    });
    if (history.length === 0) return { ...tab, history: [fallback], index: 0 };
    const keptCurrent = history.some((target) => sameTarget(target, current));
    return { ...tab, history, index: keptCurrent ? index : history.length - 1 };
  });
  return { tabs, activeId: state.activeId };
}
