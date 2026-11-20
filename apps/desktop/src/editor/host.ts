import type { EditorState } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import type { PageDoc } from "./binding.ts";

/** Where a floating menu should anchor, in viewport coordinates. */
export interface AnchorRect {
  left: number;
  top: number;
  bottom: number;
}

export type SuggestionTrigger = "slash" | "page";

/** The text before the cursor that opened a suggestion menu. */
export interface SuggestionMatch {
  trigger: SuggestionTrigger;
  /** Document position of the first trigger character. */
  from: number;
  /** Document position of the cursor. */
  to: number;
  query: string;
}

export interface SuggestionState extends SuggestionMatch {
  anchor: AnchorRect | null;
}

export type PageLinkState = "ok" | "trashed" | "missing";

export interface PageLinkInfo {
  title: string;
  state: PageLinkState;
}

/** One open view of a shared document. The editor binds to `doc`; `deliver` handlers receive updates made elsewhere. */
export interface SyncedSession {
  doc: PageDoc;
  /** Updates to the shared document that came from another device or another view of it. */
  setDeliver(handler: (update: Uint8Array) => void): void;
  close(): void;
}

/** What a synced-block node view needs from the application. */
export interface SyncedBlockHost {
  /** Open the shared document. Rejects with `code: "not_found"` while the content has not arrived. */
  open(syncId: string): Promise<SyncedSession>;
  /** How many pages show the block, or null when unknown. */
  occurrences(syncId: string): Promise<number | null>;
  /** Call `retry` when the content might have arrived; returns an unsubscribe. */
  retryWhenAvailable(syncId: string, retry: () => void): () => void;
}

export interface SyncedMenuRequest {
  syncId: string;
  /** Child indexes from the document root to the placeholder. */
  path: number[];
  anchor: AnchorRect;
}

export interface BlockMenuRequest {
  /** Top-level block indexes covered by the menu, inclusive. */
  start: number;
  end: number;
  anchor: AnchorRect;
}

export interface EditorLabels {
  toggleBlock: string;
  todoCheckbox: string;
  codeLanguage: string;
  blockHandle: string;
  imageMissing: string;
  chooseAPage: string;
  subpage: string;
  linkedPage: string;
  pageInTrash: string;
  pageMissing: string;
  editorLabel: string;
  syncedBlock: string;
  /** `{count}` is the number of pages that show the block. */
  syncedCount: string;
  syncedLoading: string;
  syncedMissing: string;
  syncedMenu: string;
  syncedNoNesting: string;
}

export const DEFAULT_LABELS: EditorLabels = {
  toggleBlock: "Show or hide the contents",
  todoCheckbox: "Done",
  codeLanguage: "Language",
  blockHandle: "Drag to move, click for options",
  imageMissing: "Image unavailable",
  chooseAPage: "Choose a page",
  subpage: "Subpage",
  linkedPage: "Link",
  pageInTrash: "In the trash",
  pageMissing: "Page not found",
  editorLabel: "Page content",
  syncedBlock: "Synced block",
  syncedCount: "Shown on {count} pages",
  syncedLoading: "Loading shared content…",
  syncedMissing: "This shared content has not arrived on this device yet.",
  syncedMenu: "Synced block options",
  syncedNoNesting: "A synced block cannot go inside another synced block.",
};

/**
 * What the editor needs from the application around it. Every member is
 * optional so the editor runs standalone (spikes, tests).
 */
export interface EditorHost {
  labels?: Partial<EditorLabels>;
  /** Live title and state of a linked page. */
  resolvePage?(pageId: string): PageLinkInfo | null;
  openPage?(pageId: string, inNewTab: boolean): void;
  openExternal?(href: string): void;
  /** A link to a stored file (`attachment:<hash>`) was clicked, for example a voice recording. */
  openAttachment?(hash: string): void;
  /** Turn an image `src` from the document into a URL the webview can load. */
  resolveImage?(src: string): string | Promise<string>;
  /** Store a pasted or dropped image; resolves to the `src` to put in the document. */
  addImage?(file: File): Promise<string>;
  /** Called after every editor state change. */
  onUpdate?(view: EditorView, previous: EditorState): void;
  /** The slash or page-link menu should open (state) or close (null). */
  onSuggest?(state: SuggestionState | null): void;
  /** Keyboard events while a suggestion menu is open; return true to consume. */
  onSuggestKey?(event: KeyboardEvent): boolean;
  onBlockMenu?(request: BlockMenuRequest): void;
  /** Shared content behind synced-block placeholders; without it they show as an unavailable block. */
  synced?: SyncedBlockHost;
  /** The options button of a synced block was used. */
  onSyncedMenu?(request: SyncedMenuRequest): void;
  /** The link shortcut was pressed; the host decides whether a link editor can open. Return true when it did. */
  onLinkShortcut?(): boolean;
  onError?(message: string): void;
}
