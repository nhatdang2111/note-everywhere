import { isIsoDate, optionNamesFor } from "../lib/db/cell-convert.ts";
import { nextOptionColor } from "../lib/db/operators.ts";
import { fold } from "../lib/text.ts";
import type {
  CellValue,
  CoreApi,
  DatabaseSchema,
  DbProperty,
  NodeId,
  PropertyType,
  QuerySpec,
  ViewDef,
  ViewKind,
} from "./contract.ts";
import { apiError } from "./errors.ts";
import { randomId, type Bus } from "./memory-bus.ts";
import { pruneSpec, runQuery, validateSpec, type EngineContext, type EngineRow } from "./memory-db-engine.ts";
import type { TreeStore } from "./memory-tree.ts";

type DbMethods = Pick<
  CoreApi,
  | "dbCreate"
  | "dbSchema"
  | "dbQuery"
  | "dbPropertyAdd"
  | "dbPropertyRename"
  | "dbPropertyDelete"
  | "dbPropertyMove"
  | "dbPropertyChangeType"
  | "dbRelationAdd"
  | "dbOptionAdd"
  | "dbOptionRename"
  | "dbOptionColor"
  | "dbOptionDelete"
  | "dbOptionMove"
  | "dbRowCreate"
  | "dbCellSet"
  | "dbCardMove"
  | "dbViewAdd"
  | "dbViewSave"
  | "dbViewDelete"
  | "dbViewMove"
>;

export interface DbStore extends DbMethods {
  /** The stored cells of a row, by property id. */
  rowCells(dbId: NodeId, rowId: NodeId): Record<string, CellValue>;
  /** Replace the stored cells of a row (templates, imports); no events. */
  setRowCells(dbId: NodeId, rowId: NodeId, cells: Record<string, CellValue>): void;
  /** A cell edit that "arrived from another device": only `tree:changed` is emitted. */
  receiveRemoteCell(dbId: NodeId, rowId: NodeId, propertyId: string, value: CellValue | null): Promise<void>;
  schemaOf(dbId: NodeId): DatabaseSchema;
  /** Add a property with a given set of select options (imports). */
  addProperty(dbId: NodeId, name: string, type: PropertyType): string;
}

interface DbState {
  props: DbProperty[];
  views: ViewDef[];
  cells: Map<NodeId, Map<string, CellValue>>;
}

const clone = <T,>(value: T): T => structuredClone(value);

function newView(name: string, kind: ViewKind, props: DbProperty[]): ViewDef {
  const first = (type: PropertyType) => props.find((prop) => prop.type === type && !prop.relation?.backlinkOf)?.id;
  const groupProperty = kind === "board" ? first("select") : undefined;
  return {
    id: randomId("view"),
    name,
    kind,
    filter: null,
    sorts: [],
    group: groupProperty ? { property: groupProperty, hideEmpty: false } : null,
    columns: [],
    dateProperty: kind === "calendar" ? (first("date") ?? null) : null,
  };
}

export function createDbStore(tree: TreeStore, bus: Bus, markDirty: () => void): DbStore {
  const states = new Map<NodeId, DbState>();

  const need = (id: NodeId): DbState => {
    const node = tree.get(id);
    if (!node || node.kind !== "database" || !tree.isLive(id)) throw apiError("not_found", "That database does not exist.");
    let state = states.get(id);
    if (!state) {
      state = { props: [], views: [newView("Table", "table", [])], cells: new Map() };
      states.set(id, state);
    }
    return state;
  };

  const needProp = (state: DbState, propertyId: string): DbProperty => {
    const prop = state.props.find((candidate) => candidate.id === propertyId);
    if (!prop) throw apiError("not_found", "That property does not exist.");
    return prop;
  };

  const needOption = (prop: DbProperty, optionId: string) => {
    const option = prop.options.find((candidate) => candidate.id === optionId);
    if (!option) throw apiError("not_found", "That option does not exist.");
    return option;
  };

  const changed = (databaseId: NodeId, rowIds: NodeId[] = []) => {
    markDirty();
    tree.touch(databaseId);
    bus.emit("database:changed", { databaseId, rowIds });
  };

  const rowsOf = (id: NodeId, state: DbState): EngineRow[] =>
    tree.liveChildren(id).flatMap((node) =>
      node.kind === "page" ? [{ id: node.id, title: node.title, icon: node.icon, raw: state.cells.get(node.id) ?? new Map() }] : [],
    );

  const needRow = (dbId: NodeId, rowId: NodeId) => {
    const node = tree.get(rowId);
    if (!node || node.parentId !== dbId || !tree.isLive(rowId)) throw apiError("not_found", "That row does not exist.");
    return node;
  };

  const contextOf = (state: DbState): EngineContext => ({
    props: state.props,
    titleOf: (id) => {
      const node = tree.get(id);
      return node && node.kind === "page" && tree.isLive(id) && tree.get(node.parentId ?? "")?.kind === "database" ? node.title : undefined;
    },
    backlinks(prop, rowId) {
      const forwardDb = prop.relation?.database;
      const forwardId = prop.relation?.backlinkOf;
      if (!forwardDb || !forwardId) return [];
      const other = states.get(forwardDb);
      if (!other) return [];
      return tree.liveChildren(forwardDb).flatMap((node) => {
        const cell = other.cells.get(node.id)?.get(forwardId);
        return cell?.type === "relation" && cell.value.includes(rowId) ? [node.id] : [];
      });
    },
  });

  function validateCell(prop: DbProperty, value: CellValue): void {
    const bad = (message: string) => apiError("invalid_input", message);
    if (value.type !== prop.type) throw bad(`A ${prop.type} property cannot hold a ${value.type} value.`);
    switch (value.type) {
      case "number":
        if (!Number.isFinite(value.value)) throw bad("A number must be finite.");
        break;
      case "select":
        needOption(prop, value.value);
        break;
      case "multi_select":
        for (const id of value.value) needOption(prop, id);
        break;
      case "date":
        if (!isIsoDate(value.value.start) || (value.value.end !== undefined && !isIsoDate(value.value.end))) throw bad("Dates are YYYY-MM-DD or YYYY-MM-DDTHH:MM.");
        break;
      default:
        break;
    }
  }

  function writeCell(state: DbState, rowId: NodeId, prop: DbProperty, value: CellValue | null): void {
    const cells = state.cells.get(rowId) ?? new Map<string, CellValue>();
    state.cells.set(rowId, cells);
    if (value === null) cells.delete(prop.id);
    else cells.set(prop.id, clone(value));
  }

  /** Writing a backlink edits the forward cells of the other database. */
  function writeBacklink(rowId: NodeId, prop: DbProperty, value: CellValue | null): void {
    const forwardDb = prop.relation?.database;
    const forwardId = prop.relation?.backlinkOf;
    const other = forwardDb ? states.get(forwardDb) : undefined;
    if (!forwardDb || !forwardId || !other) return;
    const wanted = new Set(value?.type === "relation" ? value.value : []);
    const touched: NodeId[] = [];
    for (const node of tree.liveChildren(forwardDb)) {
      const cells = other.cells.get(node.id) ?? new Map<string, CellValue>();
      const linked = cells.get(forwardId);
      const ids = linked?.type === "relation" ? linked.value : [];
      const has = ids.includes(rowId);
      if (has === wanted.has(node.id)) continue;
      const next = has ? ids.filter((id) => id !== rowId) : [...ids, rowId];
      if (next.length > 0) cells.set(forwardId, { type: "relation", value: next });
      else cells.delete(forwardId);
      other.cells.set(node.id, cells);
      touched.push(node.id);
    }
    if (touched.length > 0) bus.emit("database:changed", { databaseId: forwardDb, rowIds: touched });
  }

  async function setCell(dbId: NodeId, rowId: NodeId, propertyId: string, value: CellValue | null): Promise<void> {
    const state = need(dbId);
    needRow(dbId, rowId);
    if (propertyId === "title") {
      if (value !== null && value.type !== "text") throw apiError("invalid_input", "The title is text.");
      await tree.treeRename({ id: rowId, title: value?.value ?? "" });
      return;
    }
    const prop = needProp(state, propertyId);
    if (value !== null) validateCell(prop, value);
    if (prop.type === "relation" && prop.relation?.backlinkOf) writeBacklink(rowId, prop, value);
    else writeCell(state, rowId, prop, value);
  }

  function dropProperty(dbId: NodeId, propertyId: string): void {
    const state = states.get(dbId);
    if (!state) return;
    const at = state.props.findIndex((prop) => prop.id === propertyId);
    if (at < 0) return;
    const [removed] = state.props.splice(at, 1);
    for (const cells of state.cells.values()) cells.delete(propertyId);
    for (const view of state.views) {
      view.sorts = view.sorts.filter((sort) => sort.property !== propertyId);
      view.columns = view.columns.filter((column) => column.property !== propertyId);
      if (view.group?.property === propertyId) view.group = null;
      if (view.dateProperty === propertyId) view.dateProperty = null;
    }
    const relation = removed?.relation;
    if (relation?.reverseProperty) dropProperty(relation.database, relation.reverseProperty);
    if (relation?.backlinkOf) {
      const partner = states.get(relation.database)?.props.find((prop) => prop.id === relation.backlinkOf);
      if (partner?.relation) delete partner.relation.reverseProperty;
    }
  }

  const viewSpec = (view: ViewDef): QuerySpec => ({ filter: view.filter, sorts: view.sorts, group: view.group, dateProperty: view.dateProperty });

  const addProperty = (dbId: NodeId, name: string, type: PropertyType): string => {
    const state = need(dbId);
    if (!name.trim()) throw apiError("invalid_input", "A property needs a name.");
    const prop: DbProperty = { id: randomId("prop"), name: name.trim(), type, options: [] };
    state.props.push(prop);
    return prop.id;
  };

  return {
    schemaOf: (id) => clone({ properties: need(id).props, views: need(id).views }),
    addProperty,
    rowCells: (dbId, rowId) => Object.fromEntries(states.get(dbId)?.cells.get(rowId) ?? []),
    setRowCells(dbId, rowId, cells) {
      need(dbId).cells.set(rowId, new Map(Object.entries(clone(cells))));
    },
    async receiveRemoteCell(dbId, rowId, propertyId, value) {
      const state = need(dbId);
      needRow(dbId, rowId);
      if (propertyId === "title") {
        await tree.treeRename({ id: rowId, title: value?.type === "text" ? value.value : "" });
        return;
      }
      writeCell(state, rowId, needProp(state, propertyId), value);
      bus.emit("tree:changed", { nodeIds: [dbId, rowId] });
    },

    async dbCreate({ parentId, index, title }) {
      const id = await tree.treeCreate({ kind: "database", parentId, index, title });
      need(id);
      return id;
    },
    async dbSchema({ id }) {
      return clone({ properties: need(id).props, views: need(id).views });
    },
    async dbQuery(args) {
      const state = need(args.id);
      let spec: QuerySpec;
      if (args.spec) {
        validateSpec(args.spec, state.props);
        spec = args.spec;
      } else if (args.viewId) {
        const view = state.views.find((candidate) => candidate.id === args.viewId);
        if (!view) throw apiError("not_found", "That view does not exist.");
        spec = pruneSpec(viewSpec(view), state.props);
      } else {
        spec = {};
      }
      return clone(
        runQuery(rowsOf(args.id, state), contextOf(state), { ...spec, offset: args.offset ?? spec.offset, limit: args.limit ?? spec.limit }),
      );
    },

    async dbPropertyAdd({ id, name, type }) {
      if (type === "relation") throw apiError("invalid_input", "Add a relation with dbRelationAdd.");
      const propertyId = addProperty(id, name, type);
      changed(id);
      return propertyId;
    },
    async dbPropertyRename({ id, propertyId, name }) {
      if (!name.trim()) throw apiError("invalid_input", "A property needs a name.");
      needProp(need(id), propertyId).name = name.trim();
      changed(id);
    },
    async dbPropertyDelete({ id, propertyId }) {
      needProp(need(id), propertyId);
      dropProperty(id, propertyId);
      changed(id);
    },
    async dbPropertyMove({ id, propertyId, index }) {
      const state = need(id);
      const prop = needProp(state, propertyId);
      state.props.splice(state.props.indexOf(prop), 1);
      state.props.splice(Math.max(0, Math.min(index, state.props.length)), 0, prop);
      changed(id);
    },
    async dbPropertyChangeType({ id, propertyId, type, relationDatabaseId }) {
      const state = need(id);
      const prop = needProp(state, propertyId);
      if (prop.relation?.backlinkOf) throw apiError("invalid_input", "A backlink cannot change type.");
      if (type === "relation") {
        if (!relationDatabaseId) throw apiError("invalid_input", "Choose the database the relation points at.");
        need(relationDatabaseId);
      }
      if ((type === "select" || type === "multi_select") && (prop.type === "text" || prop.type === "url")) {
        for (const cells of state.cells.values()) {
          const cell = cells.get(propertyId);
          if (cell?.type !== "text" && cell?.type !== "url") continue;
          for (const name of optionNamesFor(cell.value, type)) {
            if (!prop.options.some((option) => fold(option.name) === fold(name))) {
              prop.options.push({ id: randomId("opt"), name, color: nextOptionColor(prop.options.length) });
            }
          }
        }
      }
      if (prop.relation?.reverseProperty) dropProperty(prop.relation.database, prop.relation.reverseProperty);
      prop.type = type;
      if (type === "relation" && relationDatabaseId) prop.relation = { database: relationDatabaseId };
      else delete prop.relation;
      changed(id);
    },
    async dbRelationAdd({ id, name, targetId, reverseName }) {
      const state = need(id);
      const target = need(targetId);
      if (!name.trim()) throw apiError("invalid_input", "A property needs a name.");
      const forward: DbProperty = { id: randomId("prop"), name: name.trim(), type: "relation", options: [], relation: { database: targetId } };
      state.props.push(forward);
      if (reverseName?.trim()) {
        const back: DbProperty = {
          id: randomId("prop"),
          name: reverseName.trim(),
          type: "relation",
          options: [],
          relation: { database: id, backlinkOf: forward.id },
        };
        target.props.push(back);
        forward.relation = { database: targetId, reverseProperty: back.id };
        if (targetId !== id) bus.emit("database:changed", { databaseId: targetId, rowIds: [] });
      }
      changed(id);
      return forward.id;
    },

    async dbOptionAdd({ id, propertyId, name, color }) {
      const prop = needProp(need(id), propertyId);
      if (!name.trim()) throw apiError("invalid_input", "An option needs a name.");
      const option = { id: randomId("opt"), name: name.trim(), color: color ?? nextOptionColor(prop.options.length) };
      prop.options.push(option);
      changed(id);
      return option.id;
    },
    async dbOptionRename({ id, propertyId, optionId, name }) {
      if (!name.trim()) throw apiError("invalid_input", "An option needs a name.");
      needOption(needProp(need(id), propertyId), optionId).name = name.trim();
      changed(id);
    },
    async dbOptionColor({ id, propertyId, optionId, color }) {
      needOption(needProp(need(id), propertyId), optionId).color = color;
      changed(id);
    },
    async dbOptionDelete({ id, propertyId, optionId }) {
      const prop = needProp(need(id), propertyId);
      prop.options.splice(prop.options.indexOf(needOption(prop, optionId)), 1);
      changed(id);
    },
    async dbOptionMove({ id, propertyId, optionId, index }) {
      const prop = needProp(need(id), propertyId);
      const option = needOption(prop, optionId);
      prop.options.splice(prop.options.indexOf(option), 1);
      prop.options.splice(Math.max(0, Math.min(index, prop.options.length)), 0, option);
      changed(id);
    },

    async dbRowCreate({ id, title, index, values }) {
      const state = need(id);
      const checked = Object.entries(values ?? {}).filter(([propertyId]) => propertyId !== "title");
      for (const [propertyId, value] of checked) validateCell(needProp(state, propertyId), value);
      const rowId = await tree.treeCreate({ kind: "page", parentId: id, index, title });
      for (const [propertyId, value] of checked) {
        const prop = needProp(state, propertyId);
        if (prop.relation?.backlinkOf) writeBacklink(rowId, prop, value);
        else writeCell(state, rowId, prop, value);
      }
      changed(id, [rowId]);
      return rowId;
    },
    async dbCellSet({ id, rowId, propertyId, value }) {
      await setCell(id, rowId, propertyId, value);
      changed(id, [rowId]);
    },
    async dbCardMove({ id, rowId, propertyId, group, beforeId }) {
      const state = need(id);
      needRow(id, rowId);
      const prop = needProp(state, propertyId);
      let value: CellValue | null;
      if (group === null) value = null;
      else if (prop.type === "select") value = { type: "select", value: group };
      else if (prop.type === "multi_select") value = { type: "multi_select", value: [group] };
      else if (prop.type === "checkbox") value = { type: "checkbox", value: group === "true" };
      else throw apiError("invalid_input", "Cards group by a select, multi-select or checkbox property.");
      await setCell(id, rowId, propertyId, value);
      const siblings = tree.liveChildren(id).filter((node) => node.id !== rowId);
      const at = beforeId ? siblings.findIndex((node) => node.id === beforeId) : -1;
      await tree.treeMove({ id: rowId, parentId: id, index: at >= 0 ? at : undefined });
      changed(id, [rowId]);
    },

    async dbViewAdd({ id, name, kind }) {
      const state = need(id);
      const view = newView(name.trim() || kind, kind, state.props);
      state.views.push(view);
      changed(id);
      return view.id;
    },
    async dbViewSave({ id, view }) {
      const state = need(id);
      const at = state.views.findIndex((candidate) => candidate.id === view.id);
      if (at < 0) throw apiError("not_found", "That view does not exist.");
      state.views[at] = clone(view);
      changed(id);
    },
    async dbViewDelete({ id, viewId }) {
      const state = need(id);
      const at = state.views.findIndex((candidate) => candidate.id === viewId);
      if (at < 0) throw apiError("not_found", "That view does not exist.");
      if (state.views.length === 1) throw apiError("invalid_input", "A database keeps at least one view.");
      state.views.splice(at, 1);
      changed(id);
    },
    async dbViewMove({ id, viewId, index }) {
      const state = need(id);
      const view = state.views.find((candidate) => candidate.id === viewId);
      if (!view) throw apiError("not_found", "That view does not exist.");
      state.views.splice(state.views.indexOf(view), 1);
      state.views.splice(Math.max(0, Math.min(index, state.views.length)), 0, view);
      changed(id);
    },
  };
}
