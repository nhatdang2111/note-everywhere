import type { DatabaseSchema, FilterNode, QuerySpec, ViewDef } from "../../api/contract.ts";
import { operatorFits } from "./operators.ts";
import { typeOfProperty } from "./filter-model.ts";

/** Query specs built from a saved view, with references to deleted properties dropped (the core rejects them in a spec). */

export function pruneFilter(filter: FilterNode | null, schema: DatabaseSchema): FilterNode | null {
  if (!filter) return null;
  if (filter.type === "condition") {
    const type = typeOfProperty(schema.properties, filter.property);
    return type && operatorFits(type, filter.operator) ? filter : null;
  }
  const children = filter.children.map((child) => pruneFilter(child, schema)).filter((child): child is FilterNode => child !== null);
  return { ...filter, children };
}

const known = (schema: DatabaseSchema, id: string) => id === "title" || schema.properties.some((prop) => prop.id === id);

/** Filter and sorts of a view, for the table and the list. */
export function flatSpec(schema: DatabaseSchema, view: ViewDef): QuerySpec {
  return { filter: pruneFilter(view.filter, schema), sorts: view.sorts.filter((sort) => known(schema, sort.property)), group: null };
}

/** Columns of a board: the view's grouping, when its property still exists and can group. */
export function boardSpec(schema: DatabaseSchema, view: ViewDef): QuerySpec {
  const type = view.group ? typeOfProperty(schema.properties, view.group.property) : undefined;
  const groupable = type === "select" || type === "multi_select" || type === "checkbox";
  return { ...flatSpec(schema, view), group: groupable ? view.group : null };
}

export function calendarSpec(schema: DatabaseSchema, view: ViewDef, from: string, to: string): QuerySpec {
  const property = view.dateProperty && schema.properties.some((prop) => prop.id === view.dateProperty && prop.type === "date") ? view.dateProperty : null;
  return { ...flatSpec(schema, view), dateProperty: property, dateRange: property ? { from, to } : null, limit: 1000 };
}
