import { readCell } from "../lib/db/cell-convert.ts";
import { vietnameseCompare } from "../lib/db/collation.ts";
import { OPERATORS, needsValue, type FilterableType } from "../lib/db/operators.ts";
import { fold } from "../lib/text.ts";
import type {
  CellValue,
  DbProperty,
  FilterNode,
  GroupConfig,
  NodeId,
  QueryBucket,
  QueryResult,
  QueryRow,
  QuerySpec,
  SortKey,
} from "./contract.ts";
import { apiError } from "./errors.ts";

/** What the query engine needs to know about a row; `raw` holds the stored cells by property id. */
export interface EngineRow {
  id: NodeId;
  title: string;
  icon: string | null;
  raw: ReadonlyMap<string, CellValue>;
}

export interface EngineContext {
  props: DbProperty[];
  /** Rows of other databases that link to `rowId` through the forward property of a backlink. */
  backlinks(prop: DbProperty, rowId: NodeId): NodeId[];
  /** Title of a row that can be shown, or undefined when it is gone or in the trash. */
  titleOf(id: NodeId): string | undefined;
}

const MAX_DEPTH = 3;
const DEFAULT_LIMIT = 100;
const MAX_LIMIT = 1000;

export function effectiveValues(row: EngineRow, ctx: EngineContext): Record<string, CellValue> {
  const values: Record<string, CellValue> = {};
  for (const prop of ctx.props) {
    let value: CellValue | undefined;
    if (prop.type === "relation" && prop.relation?.backlinkOf) {
      value = { type: "relation", value: ctx.backlinks(prop, row.id) };
    } else {
      value = readCell(prop, row.raw.get(prop.id));
    }
    if (value?.type === "relation") {
      const visible = value.value.filter((id) => ctx.titleOf(id) !== undefined);
      value = visible.length > 0 ? { type: "relation", value: visible } : undefined;
    }
    if (value) values[prop.id] = value;
  }
  return values;
}

const typeOf = (props: DbProperty[], id: string): FilterableType | undefined =>
  id === "title" ? "title" : props.find((prop) => prop.id === id)?.type;

/** Strict validation of a client-supplied spec: unknown properties and unsuitable operators are rejected. */
export function validateSpec(spec: QuerySpec, props: DbProperty[]): void {
  const check = (node: FilterNode, depth: number) => {
    if (node.type === "group") {
      if (depth > MAX_DEPTH) throw apiError("invalid_input", "Filter groups nest at most three deep.");
      for (const child of node.children) check(child, depth + 1);
      return;
    }
    const type = typeOf(props, node.property);
    if (!type) throw apiError("invalid_input", `Unknown property in filter: ${node.property}.`);
    if (!OPERATORS[type].includes(node.operator)) throw apiError("invalid_input", `The operator ${node.operator} does not suit a ${type} property.`);
  };
  if (spec.filter) check(spec.filter, 1);
  for (const sort of spec.sorts ?? []) {
    if (!typeOf(props, sort.property)) throw apiError("invalid_input", `Unknown property in sort: ${sort.property}.`);
  }
  if (spec.group) {
    const type = typeOf(props, spec.group.property);
    if (type !== "select" && type !== "multi_select" && type !== "checkbox") throw apiError("invalid_input", "Group by a select, multi-select or checkbox property.");
  }
  if (spec.dateProperty && typeOf(props, spec.dateProperty) !== "date") throw apiError("invalid_input", "The calendar property must be a date.");
}

/** Remove conditions, sorts and groupings of properties that no longer exist (used for saved views). */
export function pruneSpec(spec: QuerySpec, props: DbProperty[]): QuerySpec {
  const prune = (node: FilterNode): FilterNode | null => {
    if (node.type === "condition") {
      const type = typeOf(props, node.property);
      return type && OPERATORS[type].includes(node.operator) ? node : null;
    }
    const children = node.children.map(prune).filter((child): child is FilterNode => child !== null);
    return { ...node, children };
  };
  const group = spec.group && ["select", "multi_select", "checkbox"].includes(typeOf(props, spec.group.property) ?? "") ? spec.group : null;
  return {
    ...spec,
    filter: spec.filter ? prune(spec.filter) : null,
    sorts: (spec.sorts ?? []).filter((sort) => typeOf(props, sort.property)),
    group,
    dateProperty: spec.dateProperty && typeOf(props, spec.dateProperty) === "date" ? spec.dateProperty : null,
  };
}

const day = (text: string): string => text.slice(0, 10);

function matchCondition(operator: string, type: FilterableType, value: CellValue | undefined, title: string, wanted: string | number | boolean | undefined): boolean {
  const text = String(wanted ?? "");
  if (type === "title" || type === "text" || type === "url") {
    const actual = type === "title" ? title : value && (value.type === "text" || value.type === "url") ? value.value : "";
    const a = fold(actual);
    const b = fold(text);
    switch (operator) {
      case "contains": return a.includes(b);
      case "not_contains": return !a.includes(b);
      case "is": return a === b;
      case "is_not": return a !== b;
      case "starts_with": return a.startsWith(b);
      case "ends_with": return a.endsWith(b);
      case "is_empty": return actual === "";
      default: return actual !== "";
    }
  }
  if (type === "number") {
    const actual = value?.type === "number" ? value.value : undefined;
    const target = Number(wanted);
    switch (operator) {
      case "is_empty": return actual === undefined;
      case "is_not_empty": return actual !== undefined;
      case "is_not": return actual !== target;
      case "is": return actual === target;
      case "gt": return actual !== undefined && actual > target;
      case "gte": return actual !== undefined && actual >= target;
      case "lt": return actual !== undefined && actual < target;
      default: return actual !== undefined && actual <= target;
    }
  }
  if (type === "select") {
    const actual = value?.type === "select" ? value.value : undefined;
    switch (operator) {
      case "is": return actual === text;
      case "is_not": return actual !== text;
      case "is_empty": return actual === undefined;
      default: return actual !== undefined;
    }
  }
  if (type === "multi_select" || type === "relation") {
    const actual = value?.type === "multi_select" || value?.type === "relation" ? value.value : [];
    switch (operator) {
      case "contains": return actual.includes(text);
      case "not_contains": return !actual.includes(text);
      case "is_empty": return actual.length === 0;
      default: return actual.length > 0;
    }
  }
  if (type === "checkbox") {
    const checked = value?.type === "checkbox" && value.value;
    return operator === "is_checked" ? checked : !checked;
  }
  // date
  if (value?.type !== "date") return operator === "is_empty";
  const start = day(value.value.start);
  const last = day(value.value.end ?? value.value.start);
  switch (operator) {
    case "is": return start <= text && text <= last;
    case "before": return start < text;
    case "after": return last > text;
    case "on_or_before": return start <= text;
    case "on_or_after": return last >= text;
    case "is_empty": return false;
    default: return true;
  }
}

function matches(node: FilterNode, row: EngineRow, values: Record<string, CellValue>, props: DbProperty[]): boolean {
  if (node.type === "group") {
    if (node.children.length === 0) return true;
    return node.logic === "and"
      ? node.children.every((child) => matches(child, row, values, props))
      : node.children.some((child) => matches(child, row, values, props));
  }
  const type = typeOf(props, node.property);
  if (!type) return true;
  const wanted = needsValue(node.operator) ? node.value : undefined;
  return matchCondition(node.operator, type, values[node.property], row.title, wanted);
}

interface Entry {
  row: EngineRow;
  values: Record<string, CellValue>;
  order: number;
}

function sortKeyOf(entry: Entry, sort: SortKey, props: DbProperty[]): string | number | undefined {
  if (sort.property === "title") return entry.row.title === "" ? undefined : entry.row.title;
  const prop = props.find((candidate) => candidate.id === sort.property);
  const value = entry.values[sort.property];
  if (!prop || !value) return undefined;
  switch (value.type) {
    case "text":
    case "url":
      return value.value;
    case "number":
      return value.value;
    case "select":
      return prop.options.findIndex((option) => option.id === value.value);
    case "multi_select":
      return Math.min(...value.value.map((id) => prop.options.findIndex((option) => option.id === id)));
    case "date":
      return value.value.start;
    case "checkbox":
      return value.value ? 1 : 0;
    case "relation":
      return value.value.length;
  }
}

const compareKeys = (a: string | number, b: string | number): number =>
  typeof a === "string" && typeof b === "string" ? vietnameseCompare(a, b) : Number(a) - Number(b);

function sortEntries(entries: Entry[], sorts: SortKey[], props: DbProperty[]): Entry[] {
  if (sorts.length === 0) return entries;
  return [...entries].sort((left, right) => {
    for (const sort of sorts) {
      const a = sortKeyOf(left, sort, props);
      const b = sortKeyOf(right, sort, props);
      if (a === undefined && b === undefined) continue;
      if (a === undefined) return 1;
      if (b === undefined) return -1;
      const diff = compareKeys(a, b);
      if (diff !== 0) return sort.descending ? -diff : diff;
    }
    return left.order - right.order;
  });
}

function bucketKeys(group: GroupConfig, entry: Entry, props: DbProperty[]): (string | null)[] {
  const prop = props.find((candidate) => candidate.id === group.property);
  const value = entry.values[group.property];
  if (prop?.type === "checkbox") return [value?.type === "checkbox" && value.value ? "true" : "false"];
  if (value?.type === "select") return [value.value];
  if (value?.type === "multi_select" && value.value.length > 0) return value.value;
  return [null];
}

function toQueryRow(entry: Entry): QueryRow {
  return { id: entry.row.id, title: entry.row.title, icon: entry.row.icon, values: entry.values };
}

function touchesRange(entry: Entry, property: string, from: string, to: string): boolean {
  const value = entry.values[property];
  if (value?.type !== "date") return false;
  return day(value.value.start) <= day(to) && day(value.value.end ?? value.value.start) >= day(from);
}

export function runQuery(rows: EngineRow[], ctx: EngineContext, spec: QuerySpec): QueryResult {
  const { props } = ctx;
  const limit = Math.min(MAX_LIMIT, Math.max(1, Math.floor(spec.limit ?? DEFAULT_LIMIT)));
  const offset = Math.max(0, Math.floor(spec.offset ?? 0));

  let entries: Entry[] = rows.map((row, order) => ({ row, values: effectiveValues(row, ctx), order }));
  const filter = spec.filter;
  if (filter) entries = entries.filter((entry) => matches(filter, entry.row, entry.values, props));
  if (spec.dateProperty && spec.dateRange) {
    const { dateProperty: property, dateRange: range } = spec;
    entries = entries.filter((entry) => touchesRange(entry, property, range.from, range.to));
  }
  entries = sortEntries(entries, spec.sorts ?? [], props);

  const titles: Record<NodeId, string> = {};
  const remember = (page: Entry[]) => {
    for (const entry of page) {
      for (const value of Object.values(entry.values)) {
        if (value.type !== "relation") continue;
        for (const id of value.value) {
          const title = ctx.titleOf(id);
          if (title !== undefined) titles[id] = title;
        }
      }
    }
  };

  const group = spec.group;
  if (!group) {
    const page = entries.slice(offset, offset + limit);
    remember(page);
    return { total: entries.length, rows: page.map(toQueryRow), groups: [], titles };
  }

  const prop = props.find((candidate) => candidate.id === group.property);
  const order: (string | null)[] =
    prop?.type === "checkbox" ? ["false", "true"] : [...(prop?.options.map((option) => option.id) ?? []), null];
  const buckets = new Map<string | null, Entry[]>(order.map((key) => [key, []]));
  for (const entry of entries) {
    for (const key of bucketKeys(group, entry, props)) (buckets.get(key) ?? buckets.set(key, []).get(key))?.push(entry);
  }
  const groups: QueryBucket[] = [];
  for (const [key, members] of buckets) {
    if (group.hideEmpty && members.length === 0) continue;
    const page = members.slice(offset, offset + limit);
    remember(page);
    groups.push({ key, count: members.length, rows: page.map(toQueryRow) });
  }
  return { total: entries.length, rows: [], groups, titles };
}
