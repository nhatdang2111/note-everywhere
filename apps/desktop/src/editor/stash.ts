import { Plugin, PluginKey, type EditorState } from "prosemirror-state";

/**
 * Attributes of a block's previous kind, kept while the block is something
 * else, so heading -> to-do -> heading brings the checked state back.
 *
 * The values live in this plugin, keyed by the position of the text block that
 * carries them, and are mapped through every transaction. They are not part of
 * the Loro document (that would add an attribute to the shared schema), so
 * they last for the open page and are dropped when a remote update replaces
 * the block.
 */
export type Stash = Record<string, unknown>;

interface StashEntry {
  pos: number;
  attrs: Stash;
}

export const stashKey = new PluginKey<StashEntry[]>("ne-stash");

export function stashPlugin(): Plugin<StashEntry[]> {
  return new Plugin<StashEntry[]>({
    key: stashKey,
    state: {
      init: () => [],
      apply(tr, entries) {
        let next = entries;
        if (tr.docChanged) {
          next = entries.flatMap((entry) => {
            const mapped = tr.mapping.mapResult(entry.pos, 1);
            const node = tr.doc.nodeAt(mapped.pos);
            return mapped.deleted || mapped.deletedAfter || !node?.isTextblock ? [] : [{ pos: mapped.pos, attrs: entry.attrs }];
          });
        }
        const added = tr.getMeta(stashKey) as StashEntry[] | undefined;
        return added ? [...next.filter((entry) => !added.some((a) => a.pos === entry.pos)), ...added] : next;
      },
    },
  });
}

export function stashAt(state: EditorState, pos: number): Stash {
  return stashKey.getState(state)?.find((entry) => entry.pos === pos)?.attrs ?? {};
}

export function stashEntriesIn(state: EditorState, from: number, to: number): StashEntry[] {
  return (stashKey.getState(state) ?? []).filter((entry) => entry.pos >= from && entry.pos < to);
}

export type { StashEntry };
