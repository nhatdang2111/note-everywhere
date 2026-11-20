import type { DbProperty, FilterNode, FilterOperator } from "../../api/contract.ts";
import { OPERATORS, needsValue, operatorFits, type FilterableType } from "./operators.ts";

/**
 * The filter builder edits a tree with ids and text values; the core takes the
 * contract's `FilterNode`. `toFilter` drops what is not ready to send (a
 * condition without its value, an empty group) and `fromFilter` rebuilds the
 * editable tree from a saved filter.
 */

export type BuilderNode = BuilderGroup | BuilderCondition;

export interface BuilderGroup {
  id: string;
  type: "group";
  logic: "and" | "or";
  children: BuilderNode[];
}

export interface BuilderCondition {
  id: string;
  type: "condition";
  property: string;
  operator: FilterOperator;
  /** What the person typed or picked; numbers are parsed when the filter is built. */
  value: string;
}

/** Groups nest at most three deep, the top group included. */
export const MAX_GROUP_DEPTH = 3;

let counter = 0;
const newId = (): string => `f${(counter += 1)}`;

export const emptyGroup = (logic: "and" | "or" = "and"): BuilderGroup => ({ id: newId(), type: "group", logic, children: [] });

export const typeOfProperty = (props: DbProperty[], id: string): FilterableType | undefined =>
  id === "title" ? "title" : props.find((prop) => prop.id === id)?.type;

/** A new condition on `property`, starting at that type's first operator. */
export function newCondition(props: DbProperty[], property: string): BuilderCondition {
  const type = typeOfProperty(props, property) ?? "text";
  return { id: newId(), type: "condition", property, operator: OPERATORS[type][0] as FilterOperator, value: "" };
}

/** Change a condition's property; the operator and value reset when they no longer suit. */
export function withProperty(condition: BuilderCondition, props: DbProperty[], property: string): BuilderCondition {
  const type = typeOfProperty(props, property) ?? "text";
  const keep = operatorFits(type, condition.operator) && typeOfProperty(props, condition.property) === type;
  return { ...condition, property, operator: keep ? condition.operator : (OPERATORS[type][0] as FilterOperator), value: keep ? condition.value : "" };
}

function conditionValue(type: FilterableType, text: string): string | number | boolean | undefined {
  if (type === "number") {
    const parsed = Number(text);
    return text.trim() !== "" && Number.isFinite(parsed) ? parsed : undefined;
  }
  return text === "" ? undefined : text;
}

/** The filter to send, or null when nothing in the tree is ready. */
export function toFilter(node: BuilderNode, props: DbProperty[]): FilterNode | null {
  if (node.type === "group") {
    const children = node.children.map((child) => toFilter(child, props)).filter((child): child is FilterNode => child !== null);
    return children.length > 0 ? { type: "group", logic: node.logic, children } : null;
  }
  const type = typeOfProperty(props, node.property);
  if (!type || !operatorFits(type, node.operator)) return null;
  if (!needsValue(node.operator)) return { type: "condition", property: node.property, operator: node.operator };
  const value = conditionValue(type, node.value);
  return value === undefined ? null : { type: "condition", property: node.property, operator: node.operator, value };
}

/** The editable tree for a saved filter; the top level is always a group. */
export function fromFilter(filter: FilterNode | null): BuilderGroup {
  if (!filter) return emptyGroup();
  const build = (node: FilterNode): BuilderNode =>
    node.type === "group"
      ? { id: newId(), type: "group", logic: node.logic, children: node.children.map(build) }
      : { id: newId(), type: "condition", property: node.property, operator: node.operator, value: node.value === undefined ? "" : String(node.value) };
  const root = build(filter);
  return root.type === "group" ? root : { ...emptyGroup(), children: [root] };
}

/** Depth of the deepest group, the top one counting as 1. */
export function groupDepth(node: BuilderNode): number {
  return node.type === "group" ? 1 + Math.max(0, ...node.children.map(groupDepth)) : 0;
}

export function updateNode(root: BuilderGroup, id: string, change: (node: BuilderNode) => BuilderNode | null): BuilderGroup {
  const visit = (node: BuilderNode): BuilderNode | null => {
    if (node.id === id) return change(node);
    if (node.type !== "group") return node;
    return { ...node, children: node.children.map(visit).filter((child): child is BuilderNode => child !== null) };
  };
  const result = visit(root);
  return result?.type === "group" ? result : root;
}

export const countConditions = (node: BuilderNode): number => (node.type === "condition" ? 1 : node.children.reduce((sum, child) => sum + countConditions(child), 0));

export function findNode(root: BuilderNode, id: string): BuilderNode | undefined {
  if (root.id === id) return root;
  if (root.type !== "group") return undefined;
  for (const child of root.children) {
    const found = findNode(child, id);
    if (found) return found;
  }
  return undefined;
}
