import type { ColumnConfig, DatabaseSchema, DbProperty, ViewDef } from "../../api/contract.ts";

export const DEFAULT_COLUMN_WIDTH = 180;
export const MIN_COLUMN_WIDTH = 80;
export const MAX_COLUMN_WIDTH = 640;
export const TITLE_WIDTH = 240;

export interface Column {
  property: DbProperty;
  visible: boolean;
  width: number;
}

/** The view's columns in display order: the ones it lists first, then every other property in schema order. */
export function orderedColumns(schema: DatabaseSchema, view: ViewDef): Column[] {
  const byId = new Map(schema.properties.map((prop) => [prop.id, prop]));
  const listed = view.columns.filter((column) => byId.has(column.property));
  const rest = schema.properties.filter((prop) => !listed.some((column) => column.property === prop.id));
  return [
    ...listed.map((column) => ({ property: byId.get(column.property) as DbProperty, visible: column.visible, width: column.width ?? DEFAULT_COLUMN_WIDTH })),
    ...rest.map((property) => ({ property, visible: true, width: DEFAULT_COLUMN_WIDTH })),
  ];
}

export const visibleColumns = (schema: DatabaseSchema, view: ViewDef): Column[] => orderedColumns(schema, view).filter((column) => column.visible);

const toConfig = (columns: Column[]): ColumnConfig[] =>
  columns.map((column) => ({ property: column.property.id, visible: column.visible, width: column.width }));

/** The view with its columns replaced by `columns`. */
export const withColumns = (view: ViewDef, columns: Column[]): ViewDef => ({ ...view, columns: toConfig(columns) });

export function withColumnWidth(schema: DatabaseSchema, view: ViewDef, propertyId: string, width: number): ViewDef {
  const clamped = Math.min(MAX_COLUMN_WIDTH, Math.max(MIN_COLUMN_WIDTH, Math.round(width)));
  return withColumns(view, orderedColumns(schema, view).map((column) => (column.property.id === propertyId ? { ...column, width: clamped } : column)));
}

export function withColumnVisible(schema: DatabaseSchema, view: ViewDef, propertyId: string, visible: boolean): ViewDef {
  return withColumns(view, orderedColumns(schema, view).map((column) => (column.property.id === propertyId ? { ...column, visible } : column)));
}

/** Move a column so it sits at `index` among all columns (hidden ones included). */
export function withColumnMoved(schema: DatabaseSchema, view: ViewDef, propertyId: string, index: number): ViewDef {
  const columns = orderedColumns(schema, view);
  const from = columns.findIndex((column) => column.property.id === propertyId);
  if (from < 0) return view;
  const [moved] = columns.splice(from, 1);
  if (moved) columns.splice(Math.min(Math.max(index, 0), columns.length), 0, moved);
  return withColumns(view, columns);
}
