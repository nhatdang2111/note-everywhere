import { createSignal } from "solid-js";
import type { NodeId } from "../api/contract.ts";
import type { PageEditor } from "../editor/index.ts";
import { readJson, writeJson } from "../lib/storage.ts";

const WIDTH_KEY = "ne.sidebar.width";
export const SIDEBAR_MIN = 200;
export const SIDEBAR_MAX = 480;
export const SIDEBAR_DEFAULT = 280;

const isWidth = (value: unknown): value is number => typeof value === "number" && value >= SIDEBAR_MIN && value <= SIDEBAR_MAX;

/** A dialog any part of the app can ask the workspace to show. */
export type DialogRequest =
  | { kind: "import-structured"; parentId: NodeId | null }
  | { kind: "template-gallery"; parentId: NodeId | null }
  | { kind: "save-template"; pageId: NodeId }
  | { kind: "reminder"; pageId: NodeId; databaseId?: NodeId; propertyId?: string };

export type SidePanel = "history" | "reminders";

export interface UiStore {
  /** The dialog on top of the workspace, if any. */
  dialog: () => DialogRequest | null;
  openDialog(request: DialogRequest): void;
  closeDialog(): void;
  /** The panel at the right edge of the workspace. */
  sidePanel: () => SidePanel | null;
  /** Show a panel, or hide it when it is already the one showing. */
  toggleSidePanel(panel: SidePanel): void;
  closeSidePanel(): void;
  searchOpen: () => boolean;
  setSearchOpen(open: boolean): void;
  sidebarWidth: () => number;
  setSidebarWidth(width: number): void;
  sidebarHidden: () => boolean;
  toggleSidebar(): void;
  /** The editor of the page in view; voice input and shortcuts act on it. */
  activeEditor: () => PageEditor | null;
  setActiveEditor(editor: PageEditor | null): void;
  /** The tree row being renamed in place. */
  renamingId: () => string | null;
  setRenamingId(id: string | null): void;
  /** Bumped to ask the open page to show its find bar. */
  findRequest: () => number;
  requestFind(): void;
}

export function createUiStore(): UiStore {
  const [searchOpen, setSearchOpen] = createSignal(false);
  const [sidebarWidth, setWidth] = createSignal(readJson(WIDTH_KEY, SIDEBAR_DEFAULT, isWidth));
  const [sidebarHidden, setSidebarHidden] = createSignal(false);
  const [activeEditor, setActiveEditor] = createSignal<PageEditor | null>(null);
  const [findRequest, setFindRequest] = createSignal(0);
  const [renamingId, setRenamingId] = createSignal<string | null>(null);
  const [dialog, setDialog] = createSignal<DialogRequest | null>(null);
  const [sidePanel, setSidePanel] = createSignal<SidePanel | null>(null);
  return {
    dialog,
    openDialog: setDialog,
    closeDialog: () => setDialog(null),
    sidePanel,
    toggleSidePanel: (panel) => setSidePanel((current) => (current === panel ? null : panel)),
    closeSidePanel: () => setSidePanel(null),
    searchOpen,
    setSearchOpen,
    sidebarWidth,
    setSidebarWidth(width) {
      const clamped = Math.min(SIDEBAR_MAX, Math.max(SIDEBAR_MIN, Math.round(width)));
      setWidth(clamped);
      writeJson(WIDTH_KEY, clamped);
    },
    sidebarHidden,
    toggleSidebar: () => setSidebarHidden((hidden) => !hidden),
    activeEditor,
    setActiveEditor,
    renamingId,
    setRenamingId,
    findRequest,
    requestFind: () => setFindRequest((n) => n + 1),
  };
}
