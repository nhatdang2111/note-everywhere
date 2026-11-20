import type { Node as PmNode } from "prosemirror-model";
import { Plugin, PluginKey, TextSelection } from "prosemirror-state";
import type { Command, EditorState } from "prosemirror-state";
import type { EditorView } from "prosemirror-view";
import { createBlock, insertNode, type BlockAttrs, type BlockChoice } from "./blocks.ts";
import type { AnchorRect, EditorHost, SuggestionMatch, SuggestionState } from "./host.ts";
import { pageHref } from "./page-links.ts";
import { pageSchema } from "./schema.ts";

const PAGE_BRACKETS = /\[\[([^\]\n]*)$/;
const PAGE_AT = /(^|\s)@(\S*)$/;
const SLASH = /(^|\s)\/([^\s/]*)$/;

/** What the cursor is typing right now, if it is a trigger for the slash menu or the page picker. */
export function findSuggestion(state: EditorState): SuggestionMatch | null {
  const { selection } = state;
  if (!(selection instanceof TextSelection) || !selection.empty) return null;
  const { $from } = selection;
  if (!$from.parent.isTextblock || $from.parent.type.spec.code) return null;
  const before = $from.parent.textBetween(0, $from.parentOffset, undefined, "￼");
  const start = $from.start();

  const brackets = PAGE_BRACKETS.exec(before);
  if (brackets) return { trigger: "page", from: start + brackets.index, to: $from.pos, query: brackets[1] ?? "" };

  const at = PAGE_AT.exec(before);
  if (at) return { trigger: "page", from: start + at.index + (at[1]?.length ?? 0), to: $from.pos, query: at[2] ?? "" };

  const slash = SLASH.exec(before);
  if (slash) return { trigger: "slash", from: start + slash.index + (slash[1]?.length ?? 0), to: $from.pos, query: slash[2] ?? "" };
  return null;
}

function anchorAt(view: EditorView, pos: number): AnchorRect | null {
  try {
    const { left, top, bottom } = view.coordsAtPos(pos);
    return { left, top, bottom };
  } catch {
    return null;
  }
}

export interface SuggestionController {
  plugin: Plugin;
  /** Close the open menu and keep it closed until the trigger changes. */
  dismiss(): void;
}

const key = new PluginKey("ne-suggestion");

/**
 * Detects `/`, `[[` and `@` triggers and reports them to the host. The menu
 * itself is rendered by the application; while it is open the host also gets
 * the keyboard events it needs (arrows, Enter, Escape).
 */
export function createSuggestionController(host: EditorHost): SuggestionController {
  let current: SuggestionState | null = null;
  let dismissed: string | null = null;
  let editor: EditorView | null = null;

  const signature = (match: SuggestionMatch) => `${match.trigger}:${match.from}`;

  const publish = (view: EditorView) => {
    const match = findSuggestion(view.state);
    if (!match || signature(match) === dismissed) {
      if (!match) dismissed = null;
      if (current) host.onSuggest?.((current = null));
      return;
    }
    const next: SuggestionState = { ...match, anchor: anchorAt(view, match.from) };
    const same =
      current &&
      current.trigger === next.trigger &&
      current.from === next.from &&
      current.to === next.to &&
      current.query === next.query &&
      current.anchor?.top === next.anchor?.top &&
      current.anchor?.left === next.anchor?.left;
    if (same) return;
    current = next;
    host.onSuggest?.(next);
  };

  const plugin = new Plugin({
    key,
    view(view) {
      editor = view;
      return {
        update: (updated) => publish(updated),
        destroy() {
          if (current) host.onSuggest?.(null);
          current = null;
          editor = null;
        },
      };
    },
    props: {
      handleKeyDown: (_view, event) => (current ? (host.onSuggestKey?.(event) ?? false) : false),
    },
  });

  return {
    plugin,
    dismiss() {
      if (!current) return;
      dismissed = signature(current);
      current = null;
      host.onSuggest?.(null);
      editor?.focus();
    },
  };
}

/** Replace the trigger text with a block of the chosen kind. */
export const applyBlockSuggestion =
  (match: SuggestionMatch, choice: BlockChoice, attrs?: BlockAttrs): Command =>
  (state, dispatch, view) => {
    const tr = state.tr.delete(match.from, match.to);
    if (dispatch) dispatch(tr);
    const next = view ? view.state : state.apply(tr);
    return insertNode(createBlock(choice, attrs))(next, view ? view.dispatch : dispatch);
  };

/** Replace the trigger text with an inline link to a page. */
export const applyPageSuggestion =
  (match: SuggestionMatch, page: { id: string; title: string }): Command =>
  (state, dispatch) => {
    const title = page.title || "Untitled";
    const linked: PmNode = pageSchema.text(title, [pageSchema.marks.link.create({ href: pageHref(page.id) })]);
    if (dispatch) {
      const tr = state.tr.replaceWith(match.from, match.to, [linked, pageSchema.text(" ")]);
      dispatch(tr.scrollIntoView());
    }
    return true;
  };
