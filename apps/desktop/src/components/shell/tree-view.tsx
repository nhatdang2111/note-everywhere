import { For, Show, createMemo, createSignal, onCleanup, type JSX } from "solid-js";
import type { NodeId } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { planDrop, zoneFor, type DropZone, type TreeRow } from "../../state/tree-model.ts";
import { IconButton } from "../common/button.tsx";
import { Icon } from "../common/icons.tsx";
import { Menu, type MenuEntry } from "../common/menu.tsx";
import { RenameInput } from "./tree-rename-input.tsx";
import { treeMenuEntries } from "./tree-menu.ts";
import { EmptyState } from "../common/states.tsx";

const NODE_MIME = "application/x-ne-node";
const HOVER_OPEN_MS = 700;

interface DropTarget {
  id: NodeId;
  zone: DropZone;
}

export function TreeView(): JSX.Element {
  const app = useApp();
  const { tree, ui, tabs } = app;
  let container!: HTMLUListElement;

  const [focusedId, setFocusedId] = createSignal<NodeId | null>(null);
  const [menu, setMenu] = createSignal<{ id: NodeId; at: { x: number; y: number } } | null>(null);
  const [dragId, setDragId] = createSignal<NodeId | null>(null);
  const [dropTarget, setDropTarget] = createSignal<DropTarget | null>(null);
  let hoverTimer: ReturnType<typeof setTimeout> | undefined;
  onCleanup(() => clearTimeout(hoverTimer));

  const rowIds = createMemo(() => tree.rows().map((row) => row.node.id));
  const rowMap = createMemo(() => new Map(tree.rows().map((row) => [row.node.id, row] as const)));

  const rowFocusId = () => {
    const rows = tree.rows();
    const wanted = focusedId() ?? tabs.activePageId();
    return rows.some((row) => row.node.id === wanted) ? wanted : (rows[0]?.node.id ?? null);
  };

  const focusRow = (id: NodeId | undefined) => {
    if (!id) return;
    setFocusedId(id);
    for (const element of container.querySelectorAll<HTMLElement>('[role="treeitem"]')) {
      if (element.dataset.id === id) element.focus();
    }
  };

  const activate = (row: TreeRow, inNewTab: boolean) => {
    if (row.node.kind !== "folder") app.openPage(row.node.id, inNewTab);
    else tree.setExpanded(row.node.id);
  };

  const menuEntries = (id: NodeId): MenuEntry[] => treeMenuEntries(app, id);

  const openMenuAt = (id: NodeId, x: number, y: number) => {
    setFocusedId(id);
    setMenu({ id, at: { x, y } });
  };

  const onKeyDown = (event: KeyboardEvent) => {
    if (ui.renamingId()) return;
    const rows = tree.rows();
    const at = rows.findIndex((row) => row.node.id === rowFocusId());
    const row = rows[at];
    if (!row) return;
    const handled = () => event.preventDefault();
    switch (event.key) {
      case "ArrowDown":
        handled();
        return focusRow(rows[Math.min(at + 1, rows.length - 1)]?.node.id);
      case "ArrowUp":
        handled();
        return focusRow(rows[Math.max(at - 1, 0)]?.node.id);
      case "Home":
        handled();
        return focusRow(rows[0]?.node.id);
      case "End":
        handled();
        return focusRow(rows[rows.length - 1]?.node.id);
      case "ArrowRight":
        handled();
        if (row.hasChildren && !row.expanded) tree.setExpanded(row.node.id, true);
        else if (row.expanded) focusRow(rows[at + 1]?.node.id);
        return;
      case "ArrowLeft":
        handled();
        if (row.expanded) tree.setExpanded(row.node.id, false);
        else if (row.node.parentId) focusRow(row.node.parentId);
        return;
      case "Enter":
      case " ":
        handled();
        return activate(row, event.metaKey || event.ctrlKey);
      case "F2":
        handled();
        return ui.setRenamingId(row.node.id);
      case "Delete":
        handled();
        return void app.trashNode(row.node.id);
      case "ContextMenu": {
        handled();
        const rect = (event.target as HTMLElement).getBoundingClientRect();
        return openMenuAt(row.node.id, rect.left + 24, rect.bottom);
      }
      case "F10":
        if (!event.shiftKey) return;
        handled();
        return openMenuAt(row.node.id, (event.target as HTMLElement).getBoundingClientRect().left + 24, (event.target as HTMLElement).getBoundingClientRect().bottom);
    }
  };

  const clearDrag = () => {
    clearTimeout(hoverTimer);
    setDragId(null);
    setDropTarget(null);
  };

  const onDragOver = (event: DragEvent, row: TreeRow) => {
    const dragged = dragId();
    if (!dragged) return;
    const rect = (event.currentTarget as HTMLElement).getBoundingClientRect();
    const zone = zoneFor(event.clientY - rect.top, rect.height);
    if (!planDrop(tree.index(), dragged, row.node.id, zone)) {
      // Not allowed (onto itself or its own descendant): no indicator, no drop.
      if (event.dataTransfer) event.dataTransfer.dropEffect = "none";
      setDropTarget(null);
      return;
    }
    event.preventDefault();
    if (event.dataTransfer) event.dataTransfer.dropEffect = "move";
    const current = dropTarget();
    if (current?.id !== row.node.id || current.zone !== zone) {
      setDropTarget({ id: row.node.id, zone });
      clearTimeout(hoverTimer);
      if (zone === "inside" && row.hasChildren && !row.expanded) {
        hoverTimer = setTimeout(() => tree.setExpanded(row.node.id, true), HOVER_OPEN_MS);
      }
    }
  };

  const onDrop = (event: DragEvent, row: TreeRow) => {
    const dragged = dragId() ?? event.dataTransfer?.getData(NODE_MIME);
    const target = dropTarget();
    clearDrag();
    if (!dragged || !target) return;
    const plan = planDrop(tree.index(), dragged, row.node.id, target.zone);
    if (!plan) return;
    event.preventDefault();
    void app.guard(() => tree.move(plan));
  };

  /** Dropping on the empty area below the list moves the node to the top level, last. */
  const onTailDrop = (event: DragEvent) => {
    const dragged = dragId();
    clearDrag();
    if (!dragged) return;
    event.preventDefault();
    if (tree.index().byId.get(dragged)?.parentId === null && tree.index().children.get(null)?.at(-1) === dragged) return;
    void app.guard(() => tree.move({ id: dragged, parentId: null }));
  };

  return (
    <>
      <Show
        when={tree.rows().length > 0 || !tree.loaded()}
        fallback={
          <EmptyState icon="page" title={t("tree.emptyTitle")} action={undefined}>
            {t("tree.emptyBody")}
          </EmptyState>
        }
      >
        <ul
          ref={container}
          class="tree"
          role="tree"
          aria-label={t("tree.label")}
          onKeyDown={onKeyDown}
          onDragOver={(event) => {
            if (dragId() && event.target === event.currentTarget) event.preventDefault();
          }}
          onDrop={(event) => event.target === event.currentTarget && onTailDrop(event)}
        >
          <For each={rowIds()}>
            {(id) => {
              // Rows are looked up by id so a refresh of the tree updates them in
              // place instead of rebuilding the DOM (which would drop focus and drag state).
              let last = rowMap().get(id) as TreeRow;
              const row = () => (last = rowMap().get(id) ?? last);
              const active = () => tabs.activePageId() === id;
              const rename = () => ui.renamingId() === id;
              const drop = () => (dropTarget()?.id === id ? dropTarget()?.zone : undefined);
              return (
                <li role="none">
                  <div
                    role="treeitem"
                    class="tree-row"
                    data-id={id}
                    data-kind={row().node.kind}
                    data-drop={drop()}
                    data-dragging={dragId() === id ? "true" : undefined}
                    aria-level={row().depth + 1}
                    aria-expanded={row().hasChildren ? row().expanded : undefined}
                    aria-selected={active()}
                    aria-current={active() ? "page" : undefined}
                    tabindex={rowFocusId() === id ? 0 : -1}
                    style={{ "padding-left": `${row().depth * 16 + 4}px` }}
                    draggable={!rename()}
                    onFocus={() => setFocusedId(id)}
                    onClick={(event) => activate(row(), event.metaKey || event.ctrlKey)}
                    onAuxClick={(event) => event.button === 1 && row().node.kind !== "folder" && app.openPage(id, true)}
                    onDblClick={() => ui.setRenamingId(id)}
                    onContextMenu={(event) => {
                      event.preventDefault();
                      openMenuAt(id, event.clientX, event.clientY);
                    }}
                    onDragStart={(event) => {
                      event.dataTransfer?.setData(NODE_MIME, id);
                      if (event.dataTransfer) event.dataTransfer.effectAllowed = "move";
                      setDragId(id);
                    }}
                    onDragEnd={clearDrag}
                    onDragOver={(event) => onDragOver(event, row())}
                    onDragLeave={(event) => {
                      if (!(event.currentTarget as HTMLElement).contains(event.relatedTarget as Node | null)) setDropTarget(null);
                    }}
                    onDrop={(event) => onDrop(event, row())}
                  >
                    <button
                      type="button"
                      class="tree-toggle"
                      tabindex="-1"
                      aria-hidden={row().hasChildren ? undefined : "true"}
                      data-visible={row().hasChildren ? "true" : "false"}
                      aria-label={row().expanded ? t("tree.collapse") : t("tree.expand")}
                      onClick={(event) => {
                        event.stopPropagation();
                        tree.setExpanded(id);
                      }}
                    >
                      <Icon name={row().expanded ? "chevron-down" : "chevron-right"} size={14} />
                    </button>
                    <span class="tree-icon">
                      {row().node.icon ? <span aria-hidden="true">{row().node.icon}</span> : <Icon name={row().node.kind === "folder" ? "folder" : row().node.kind === "database" ? "database" : "page"} />}
                    </span>
                    <Show
                      when={!rename()}
                      fallback={
                        <RenameInput
                          initial={row().node.title}
                          onCommit={(title) => {
                            ui.setRenamingId(null);
                            if (title !== row().node.title) void app.guard(() => tree.rename(id, title));
                            focusRow(id);
                          }}
                          onCancel={() => {
                            ui.setRenamingId(null);
                            focusRow(id);
                          }}
                        />
                      }
                    >
                      <span class="tree-title" data-empty={row().node.title ? undefined : "true"}>
                        {row().node.title || t("tree.untitled")}
                      </span>
                    </Show>
                    <span class="tree-actions">
                      <IconButton
                        icon="plus"
                        size={14}
                        label={t("tree.newPageInside")}
                        tabindex={-1}
                        onClick={(event) => {
                          event.stopPropagation();
                          void app.createNode("page", id);
                        }}
                      />
                      <IconButton
                        icon="more"
                        size={14}
                        label={t("tree.more")}
                        tabindex={-1}
                        onClick={(event) => {
                          event.stopPropagation();
                          const rect = event.currentTarget.getBoundingClientRect();
                          openMenuAt(id, rect.left, rect.bottom + 4);
                        }}
                      />
                    </span>
                  </div>
                </li>
              );
            }}
          </For>
        </ul>
      </Show>
      <Show when={menu()}>{(open) => <Menu label={t("tree.menuLabel")} at={open().at} entries={menuEntries(open().id)} onClose={() => setMenu(null)} />}</Show>
    </>
  );
}
