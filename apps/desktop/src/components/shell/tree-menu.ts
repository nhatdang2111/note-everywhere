import { t } from "../../i18n/index.ts";
import type { NodeId } from "../../api/contract.ts";
import type { App } from "../../state/app.tsx";
import { planStep } from "../../state/tree-model.ts";
import type { MenuEntry } from "../common/menu.tsx";

const move = (app: App, plan: ReturnType<typeof planStep>) => {
  if (plan) void app.guard(() => app.tree.move(plan));
};

/** Context menu of a tree row. Move entries are disabled when the move would change nothing. */
export function treeMenuEntries(app: App, id: NodeId): MenuEntry[] {
  const { tree, ui } = app;
  const node = tree.index().byId.get(id);
  if (!node) return [];
  const step = (direction: "up" | "down" | "out") => planStep(tree.index(), id, direction);
  const entries: MenuEntry[] = [];
  if (node.kind === "page") {
    entries.push({ kind: "item", icon: "page", label: t("tree.open"), onSelect: () => app.openPage(id) });
    entries.push({ kind: "item", icon: "plus", label: t("tree.openInNewTab"), onSelect: () => app.openPage(id, true) });
    entries.push({ kind: "separator" });
  }
  entries.push(
    { kind: "item", icon: "page", label: t("tree.newPageInside"), onSelect: () => void app.createNode("page", id) },
    { kind: "item", icon: "folder", label: t("tree.newFolderInside"), onSelect: () => void app.createNode("folder", id) },
    { kind: "separator" },
    { kind: "item", icon: "edit", label: t("tree.rename"), onSelect: () => ui.setRenamingId(id) },
    {
      kind: "item",
      icon: "star",
      label: node.favorite ? t("tree.removeFavorite") : t("tree.addFavorite"),
      onSelect: () => void app.guard(() => tree.setFavorite(id, !node.favorite)),
    },
    { kind: "separator" },
    { kind: "item", icon: "arrow-up", label: t("tree.moveUp"), disabled: !step("up"), onSelect: () => move(app, step("up")) },
    { kind: "item", icon: "arrow-down", label: t("tree.moveDown"), disabled: !step("down"), onSelect: () => move(app, step("down")) },
    { kind: "item", icon: "arrow-left", label: t("tree.moveOut"), disabled: !step("out"), onSelect: () => move(app, step("out")) },
    { kind: "separator" },
    { kind: "item", icon: "upload", label: t("tree.exportMarkdown"), onSelect: () => void app.exportMarkdown(id) },
    { kind: "item", icon: "download", label: t("tree.importMarkdownHere"), onSelect: () => void app.importMarkdown(id) },
    { kind: "separator" },
    { kind: "item", icon: "trash", danger: true, label: t("tree.moveToTrash"), onSelect: () => void app.trashNode(id) },
  );
  return entries;
}
