import { createSignal } from "solid-js";
import type { NodeId } from "../api/contract.ts";
import { readJson, writeJson } from "../lib/storage.ts";
import {
  activate as activateTab,
  activeTab,
  canGo,
  closeTab,
  currentTarget,
  go,
  initialTabs,
  openNewEmptyTab,
  openTarget,
  pruneMissing,
  type Tab,
  type TabsState,
  type Target,
} from "./tabs-model.ts";

const TABS_KEY = "ne.tabs";
const HOME: Target = { kind: "home" };

function isTabsState(value: unknown): value is TabsState {
  if (typeof value !== "object" || value === null) return false;
  const { tabs, activeId } = value as Partial<TabsState>;
  return (
    Array.isArray(tabs) &&
    tabs.length > 0 &&
    typeof activeId === "string" &&
    tabs.every((tab) => typeof tab?.id === "string" && Array.isArray(tab.history) && tab.history.length > 0 && typeof tab.index === "number")
  );
}

export interface TabsStore {
  tabs: () => Tab[];
  activeId: () => string;
  target: () => Target;
  /** The page shown in the active tab, or null. */
  activePageId: () => NodeId | null;
  open(target: Target, options?: { newTab?: boolean }): void;
  newTab(): void;
  activate(id: string): void;
  close(id: string): void;
  closeActive(): void;
  back(): void;
  forward(): void;
  canBack: () => boolean;
  canForward: () => boolean;
  /** Forget pages that no longer exist so no tab or history entry points at them. */
  prune(exists: (pageId: NodeId) => boolean): void;
}

export function createTabsStore(): TabsStore {
  const [state, setState] = createSignal<TabsState>(readJson<TabsState>(TABS_KEY, initialTabs(), isTabsState));
  const update = (next: TabsState) => {
    setState(next);
    writeJson(TABS_KEY, next);
  };

  return {
    tabs: () => state().tabs,
    activeId: () => state().activeId,
    target: () => currentTarget(activeTab(state())),
    activePageId() {
      const target = currentTarget(activeTab(state()));
      return target.kind === "page" ? target.pageId : null;
    },
    open: (target, options) => update(openTarget(state(), target, options?.newTab)),
    newTab: () => update(openNewEmptyTab(state(), HOME)),
    activate: (id) => update(activateTab(state(), id)),
    close: (id) => update(closeTab(state(), id, HOME)),
    closeActive: () => update(closeTab(state(), state().activeId, HOME)),
    back: () => update(go(state(), -1)),
    forward: () => update(go(state(), 1)),
    canBack: () => canGo(state(), -1),
    canForward: () => canGo(state(), 1),
    prune: (exists) => {
      const next = pruneMissing(state(), exists, HOME);
      if (JSON.stringify(next) !== JSON.stringify(state())) update(next);
    },
  };
}
