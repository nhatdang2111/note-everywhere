import type { DbProperty, ViewDef } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { orderedColumns, withColumnMoved, withColumnVisible } from "../../lib/db/view-model.ts";
import type { DatabaseController } from "../../state/database-store.ts";
import type { MenuEntry } from "../common/menu.tsx";
import type { DatabaseActions } from "./database-context.tsx";

/** Sort by `property` first, replacing any earlier sort on it. */
export function withPrimarySort(view: ViewDef, property: string, descending: boolean): ViewDef {
  return { ...view, sorts: [{ property, descending }, ...view.sorts.filter((sort) => sort.property !== property)] };
}

/** Menu of a column header: sorting, visibility, order and schema changes. `property` is "title" for the title column. */
export function propertyMenuEntries(controller: DatabaseController, actions: DatabaseActions, property: DbProperty | "title"): MenuEntry[] {
  const schema = controller.schema();
  const view = controller.view();
  if (!schema || !view) return [];
  const id = property === "title" ? "title" : property.id;
  const columns = orderedColumns(schema, view);
  const at = columns.findIndex((column) => column.property.id === id);
  const entries: MenuEntry[] = [
    { kind: "item", icon: "arrow-up", label: t("db.sortAscending"), onSelect: () => void controller.saveView(withPrimarySort(view, id, false)) },
    { kind: "item", icon: "arrow-down", label: t("db.sortDescending"), onSelect: () => void controller.saveView(withPrimarySort(view, id, true)) },
  ];
  if (property === "title") return entries;
  entries.push(
    { kind: "separator" },
    { kind: "item", icon: "edit", label: t("db.renameProperty"), onSelect: () => actions.openSchemaDialog({ kind: "rename-property", propertyId: id }) },
    {
      kind: "item",
      icon: "settings",
      label: t("db.changeType"),
      disabled: Boolean(property.relation?.backlinkOf),
      onSelect: () => actions.openSchemaDialog({ kind: "change-type", propertyId: id }),
    },
  );
  if (property.type === "select" || property.type === "multi_select") {
    entries.push({ kind: "item", icon: "bullet-list", label: t("db.manageOptions"), onSelect: () => actions.openSchemaDialog({ kind: "options", propertyId: id }) });
  }
  entries.push(
    { kind: "separator" },
    { kind: "item", icon: "eye-off", label: t("db.hideProperty"), onSelect: () => void controller.saveView(withColumnVisible(schema, view, id, false)) },
    {
      kind: "item",
      icon: "arrow-left",
      label: t("db.moveLeft"),
      disabled: at <= 0,
      onSelect: () => void controller.saveView(withColumnMoved(schema, view, id, at - 1)),
    },
    {
      kind: "item",
      icon: "arrow-right",
      label: t("db.moveRight"),
      disabled: at < 0 || at >= columns.length - 1,
      onSelect: () => void controller.saveView(withColumnMoved(schema, view, id, at + 1)),
    },
    { kind: "separator" },
    { kind: "item", icon: "trash", danger: true, label: t("db.deleteProperty"), onSelect: () => actions.openSchemaDialog({ kind: "delete-property", propertyId: id }) },
  );
  return entries;
}
