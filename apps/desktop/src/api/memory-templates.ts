import { LoroDoc } from "loro-crdt";
import type { CellValue, CoreApi, NodeId, TemplateSummary } from "./contract.ts";
import { apiError } from "./errors.ts";
import { randomId } from "./memory-bus.ts";
import type { DbStore } from "./memory-db.ts";
import type { TreeStore } from "./memory-tree.ts";
import { createSkeleton } from "./page-doc.ts";

type TemplateMethods = Pick<
  CoreApi,
  | "templateList"
  | "templateCreateFromPage"
  | "templateCreateForDatabase"
  | "templateRename"
  | "templateSetDefaults"
  | "templateDelete"
  | "templateInstantiatePage"
  | "templateInstantiateRow"
>;

export type Templates = TemplateMethods;

interface Hooks {
  tree: TreeStore;
  db: DbStore;
  doc(id: NodeId): LoroDoc | undefined;
  replaceDoc(id: NodeId, doc: LoroDoc): void;
  now(): number;
  markDirty(): void;
}

interface Stored {
  summary: TemplateSummary;
  /** Title the page or row had when the template was made. */
  title: string;
  snapshot: Uint8Array;
}

function cloneDoc(snapshot: Uint8Array): LoroDoc {
  const doc = new LoroDoc();
  doc.setRecordTimestamp(true);
  doc.import(snapshot);
  return doc;
}

export function createTemplates(hooks: Hooks): Templates {
  const store = new Map<string, Stored>();
  const { tree, db } = hooks;

  const need = (id: string): Stored => {
    const found = store.get(id);
    if (!found) throw apiError("not_found", "That template does not exist.");
    return found;
  };

  const snapshotOf = (pageId: NodeId): Uint8Array => (hooks.doc(pageId) ?? createSkeleton()).export({ mode: "snapshot" });

  const add = (summary: Omit<TemplateSummary, "id" | "createdAt">, title: string, snapshot: Uint8Array): string => {
    if (!summary.name.trim()) throw apiError("invalid_input", "A template needs a name.");
    const id = randomId("tpl");
    store.set(id, { summary: { ...summary, name: summary.name.trim(), id, createdAt: hooks.now() }, title, snapshot });
    hooks.markDirty();
    return id;
  };

  const livePage = (id: NodeId) => {
    const node = tree.get(id);
    if (!node || node.kind !== "page" || !tree.isLive(id)) throw apiError("not_found", "That page is not available.");
    return node;
  };

  /** Defaults that still fit the schema: a property or option deleted since is dropped. */
  function usableDefaults(databaseId: NodeId, defaults: Record<string, CellValue>): Record<string, CellValue> {
    const { properties } = db.schemaOf(databaseId);
    const usable: Record<string, CellValue> = {};
    for (const [propertyId, value] of Object.entries(defaults)) {
      const prop = properties.find((candidate) => candidate.id === propertyId);
      if (!prop || prop.type !== value.type || prop.relation?.backlinkOf) continue;
      const known = (optionId: string) => prop.options.some((option) => option.id === optionId);
      if (value.type === "select" && !known(value.value)) continue;
      if (value.type === "multi_select") {
        const kept = value.value.filter(known);
        if (kept.length > 0) usable[propertyId] = { type: "multi_select", value: kept };
        continue;
      }
      usable[propertyId] = value;
    }
    return usable;
  }

  return {
    async templateList({ databaseId }) {
      return [...store.values()]
        .map((entry) => entry.summary)
        .filter((summary) => (databaseId ? summary.kind === "row" && summary.database === databaseId : summary.kind === "page"))
        .map((summary) => structuredClone(summary));
    },
    async templateCreateFromPage({ pageId, name }) {
      const node = livePage(pageId);
      return add({ name, kind: "page", defaults: {} }, node.title, snapshotOf(pageId));
    },
    async templateCreateForDatabase({ databaseId, name, fromRowId }) {
      db.schemaOf(databaseId);
      if (!fromRowId) return add({ name, kind: "row", database: databaseId, defaults: {} }, "", createSkeleton().export({ mode: "snapshot" }));
      const row = livePage(fromRowId);
      if (row.parentId !== databaseId) throw apiError("invalid_input", "That row belongs to another database.");
      const { properties } = db.schemaOf(databaseId);
      const defaults = Object.fromEntries(
        Object.entries(db.rowCells(databaseId, fromRowId)).filter(([propertyId]) => properties.find((prop) => prop.id === propertyId)?.type !== "relation"),
      );
      return add({ name, kind: "row", database: databaseId, defaults }, row.title, snapshotOf(fromRowId));
    },
    async templateRename({ id, name }) {
      if (!name.trim()) throw apiError("invalid_input", "A template needs a name.");
      need(id).summary.name = name.trim();
    },
    async templateSetDefaults({ id, defaults }) {
      const entry = need(id);
      if (entry.summary.kind !== "row") throw apiError("invalid_input", "Only row templates have default values.");
      entry.summary.defaults = structuredClone(defaults);
    },
    async templateDelete({ id }) {
      need(id);
      store.delete(id);
    },
    async templateInstantiatePage({ id, parentId, index, title }) {
      const entry = need(id);
      if (entry.summary.kind !== "page") throw apiError("invalid_input", "That is a row template.");
      const pageId = await tree.treeCreate({ kind: "page", parentId, index, title: title ?? entry.title });
      hooks.replaceDoc(pageId, cloneDoc(entry.snapshot));
      return pageId;
    },
    async templateInstantiateRow({ id, title }) {
      const entry = need(id);
      const databaseId = entry.summary.database;
      if (entry.summary.kind !== "row" || !databaseId) throw apiError("invalid_input", "That is a page template.");
      const rowId = await db.dbRowCreate({ id: databaseId, title: title ?? entry.title, values: usableDefaults(databaseId, entry.summary.defaults) });
      hooks.replaceDoc(rowId, cloneDoc(entry.snapshot));
      return rowId;
    },
  };
}
