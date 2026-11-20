import { LoroDoc } from "loro-crdt";
import { LoroSyncPlugin, LoroUndoPlugin, redo, undo, type LoroDocType } from "loro-prosemirror";
import { keymap } from "prosemirror-keymap";
import type { Plugin } from "prosemirror-state";

/**
 * Loro document type the binding expects. The binding only reads and writes
 * the root map `doc`; see docs/decisions/0006-page-document-structure.md.
 */
export type PageDoc = LoroDocType;

/** Changes by one author closer together than this (seconds) are stored as one change. */
export const CHANGE_MERGE_INTERVAL_SECONDS = 30;

/**
 * Make the document's changes carry a time and merge a typing burst into one
 * change, which is what page history groups versions from. Every document an
 * editor binds to goes through this, so it is safe to call more than once.
 */
export function configureDoc<T extends PageDoc>(doc: T): T {
  doc.setRecordTimestamp(true);
  doc.setChangeMergeInterval(CHANGE_MERGE_INTERVAL_SECONDS);
  return doc;
}

/** A fresh, empty page document. */
export function createPageDoc(): PageDoc {
  // The binding types its document as LoroDoc<{doc, data}>; a plain LoroDoc has
  // the same runtime shape and the container map is resolved lazily by name.
  return configureDoc(new LoroDoc() as unknown as PageDoc);
}

/**
 * Plugins that bind a ProseMirror editor to a Loro document.
 *
 * Undo/redo comes from LoroUndoPlugin, so it only reverts this device's own
 * edits and never touches changes imported from another device.
 *
 * This module has no DOM dependency so the headless spikes can reuse it.
 */
export function bindingPlugins(doc: PageDoc): Plugin[] {
  configureDoc(doc);
  return [
    LoroSyncPlugin({ doc }),
    LoroUndoPlugin({ doc }),
    keymap({
      "Mod-z": undo,
      "Mod-y": redo,
      "Mod-Shift-z": redo,
    }),
  ];
}
