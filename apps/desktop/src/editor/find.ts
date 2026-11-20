import type { Node as PmNode } from "prosemirror-model";
import { Plugin, PluginKey, TextSelection } from "prosemirror-state";
import type { EditorState } from "prosemirror-state";
import { Decoration, DecorationSet } from "prosemirror-view";
import type { EditorView } from "prosemirror-view";
import { findAll } from "../lib/text.ts";

export interface FindMatch {
  from: number;
  to: number;
}

export interface FindState {
  query: string;
  matches: FindMatch[];
  /** Index into `matches`, or -1 when there are none. */
  current: number;
}

type FindMeta = { query: string } | { step: 1 | -1 } | { clear: true };

const key = new PluginKey<FindState>("ne-find");
const EMPTY: FindState = { query: "", matches: [], current: -1 };

/** Matches in reading order. Case and accents are ignored; offsets stay valid because folding is 1:1. */
export function findMatches(doc: PmNode, query: string): FindMatch[] {
  if (!query) return [];
  const matches: FindMatch[] = [];
  doc.descendants((node, pos) => {
    if (!node.isTextblock) return true;
    const text = node.textBetween(0, node.content.size, "\n", "￼");
    for (const at of findAll(text, query)) {
      matches.push({ from: pos + 1 + at, to: pos + 1 + at + query.length });
    }
    return false;
  });
  return matches;
}

export function findPlugin(): Plugin<FindState> {
  return new Plugin<FindState>({
    key,
    state: {
      init: () => EMPTY,
      apply(tr, value) {
        const meta = tr.getMeta(key) as FindMeta | undefined;
        if (meta && "clear" in meta) return EMPTY;
        if (meta && "query" in meta) {
          const matches = findMatches(tr.doc, meta.query);
          return { query: meta.query, matches, current: matches.length > 0 ? 0 : -1 };
        }
        if (meta && "step" in meta) {
          if (value.matches.length === 0) return value;
          const count = value.matches.length;
          return { ...value, current: (value.current + meta.step + count) % count };
        }
        if (tr.docChanged && value.query) {
          const matches = findMatches(tr.doc, value.query);
          return { ...value, matches, current: Math.min(Math.max(value.current, 0), matches.length - 1) };
        }
        return value;
      },
    },
    props: {
      decorations(state) {
        const value = key.getState(state);
        if (!value || value.matches.length === 0) return null;
        return DecorationSet.create(
          state.doc,
          value.matches.map((match, index) =>
            Decoration.inline(match.from, match.to, { class: index === value.current ? "ne-find-match ne-find-current" : "ne-find-match" }),
          ),
        );
      },
    },
  });
}

export const getFindState = (state: EditorState): FindState => key.getState(state) ?? EMPTY;

/** Search for `query`; selects and reveals the first match. */
export function setFindQuery(view: EditorView, query: string): void {
  view.dispatch(view.state.tr.setMeta(key, { query } satisfies FindMeta));
  revealCurrent(view);
}

export function stepFind(view: EditorView, step: 1 | -1): void {
  view.dispatch(view.state.tr.setMeta(key, { step } satisfies FindMeta));
  revealCurrent(view);
}

export function clearFind(view: EditorView): void {
  view.dispatch(view.state.tr.setMeta(key, { clear: true } satisfies FindMeta));
}

function revealCurrent(view: EditorView): void {
  const { matches, current } = getFindState(view.state);
  const match = matches[current];
  if (!match) return;
  view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, match.from, match.to)).scrollIntoView());
}
