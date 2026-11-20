import type { EditorState } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";

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
  /** The link shortcut was pressed; the host decides whether a link editor can open. Return true when it did. */
  onLinkShortcut?(): boolean;
  onError?(message: string): void;
}
