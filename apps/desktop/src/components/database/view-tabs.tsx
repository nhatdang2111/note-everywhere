import { For, Show, createSignal, type JSX } from "solid-js";
import type { ViewKind } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { IconButton } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";
import { Icon, type IconName } from "../common/icons.tsx";
import { Menu, type MenuEntry } from "../common/menu.tsx";
import { useDatabase } from "./database-context.tsx";

export const VIEW_KINDS: ViewKind[] = ["table", "board", "list", "calendar"];
export const VIEW_ICONS: Record<ViewKind, IconName> = { table: "paragraph", board: "toggle", list: "bullet-list", calendar: "callout" };

/** The views of a database as tabs: switch, add, rename, duplicate, delete and reorder. */
export function ViewTabs(): JSX.Element {
  const db = useDatabase();
  const [menu, setMenu] = createSignal<{ kind: "view" | "add"; at: { x: number; y: number } } | null>(null);
  const [renaming, setRenaming] = createSignal<string | null>(null);
  const [deleting, setDeleting] = createSignal<string | null>(null);
  const views = () => db.schema()?.views ?? [];
  const activeId = () => db.view()?.id;

  const commitRename = (viewId: string, name: string) => {
    setRenaming(null);
    const view = views().find((candidate) => candidate.id === viewId);
    if (view && name.trim() && name.trim() !== view.name) void db.saveView({ ...view, name: name.trim() });
  };

  const viewEntries = (): MenuEntry[] => {
    const id = activeId();
    const at = views().findIndex((view) => view.id === id);
    if (!id) return [];
    return [
      { kind: "item", icon: "edit", label: t("db.renameView"), onSelect: () => setRenaming(id) },
      { kind: "item", icon: "copy", label: t("db.duplicateView"), onSelect: () => void db.duplicateView(id) },
      { kind: "item", icon: "arrow-left", label: t("db.moveViewLeft"), disabled: at <= 0, onSelect: () => void db.moveView(id, at - 1) },
      { kind: "item", icon: "arrow-right", label: t("db.moveViewRight"), disabled: at >= views().length - 1, onSelect: () => void db.moveView(id, at + 1) },
      { kind: "separator" },
      { kind: "item", icon: "trash", danger: true, label: t("db.deleteView"), disabled: views().length <= 1, onSelect: () => setDeleting(id) },
    ];
  };

  const addEntries = (): MenuEntry[] =>
    VIEW_KINDS.map((kind) => ({ kind: "item" as const, icon: VIEW_ICONS[kind], label: t(`db.view.${kind}`), onSelect: () => void db.addView(kind, t(`db.view.${kind}`)) }));

  const onKeyDown = (event: KeyboardEvent) => {
    const list = views();
    const at = list.findIndex((view) => view.id === activeId());
    const go = (to: number) => {
      event.preventDefault();
      const next = list[(to + list.length) % list.length];
      if (next) db.setActiveView(next.id);
    };
    if (event.key === "ArrowRight") go(at + 1);
    else if (event.key === "ArrowLeft") go(at - 1);
    else if (event.key === "F2" && activeId()) (event.preventDefault(), setRenaming(activeId() as string));
  };

  return (
    <div class="view-tabs">
      <div role="tablist" aria-label={t("db.viewsLabel")} onKeyDown={onKeyDown}>
        <For each={views()}>
          {(view) => (
            <div class="view-tab" data-selected={view.id === activeId() ? "true" : "false"}>
              <Show
                when={renaming() === view.id}
                fallback={
                  <button
                    type="button"
                    role="tab"
                    aria-selected={view.id === activeId()}
                    tabindex={view.id === activeId() ? 0 : -1}
                    onClick={() => db.setActiveView(view.id)}
                    onDblClick={() => setRenaming(view.id)}
                  >
                    <Icon name={VIEW_ICONS[view.kind]} size={14} />
                    <span>{view.name}</span>
                  </button>
                }
              >
                <input
                  class="view-rename"
                  aria-label={t("db.renameView")}
                  value={view.name}
                  ref={(element) => queueMicrotask(() => (element.focus(), element.select()))}
                  onKeyDown={(event) => {
                    event.stopPropagation();
                    if (event.key === "Enter") commitRename(view.id, event.currentTarget.value);
                    else if (event.key === "Escape") setRenaming(null);
                  }}
                  onBlur={(event) => renaming() === view.id && commitRename(view.id, event.currentTarget.value)}
                />
              </Show>
            </div>
          )}
        </For>
      </div>
      <IconButton
        icon="more"
        size={14}
        label={t("db.viewMenu")}
        onClick={(event) => {
          const rect = event.currentTarget.getBoundingClientRect();
          setMenu({ kind: "view", at: { x: rect.left, y: rect.bottom + 4 } });
        }}
      />
      <IconButton
        icon="plus"
        size={14}
        label={t("db.addView")}
        onClick={(event) => {
          const rect = event.currentTarget.getBoundingClientRect();
          setMenu({ kind: "add", at: { x: rect.left, y: rect.bottom + 4 } });
        }}
      />
      <Show when={menu()}>{(open) => <Menu label={t(open().kind === "add" ? "db.addView" : "db.viewMenu")} at={open().at} entries={open().kind === "add" ? addEntries() : viewEntries()} onClose={() => setMenu(null)} />}</Show>
      <Show when={deleting()}>
        {(id) => (
          <ConfirmDialog
            title={t("db.deleteViewTitle")}
            message={t("db.deleteViewBody", { name: views().find((view) => view.id === id())?.name ?? "" })}
            confirmLabel={t("common.delete")}
            danger
            onCancel={() => setDeleting(null)}
            onConfirm={() => (void db.deleteView(id()), setDeleting(null))}
          />
        )}
      </Show>
    </div>
  );
}
