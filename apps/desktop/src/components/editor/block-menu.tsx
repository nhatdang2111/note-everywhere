import type { JSX } from "solid-js";
import type { Command } from "prosemirror-state";
import {
  TURN_INTO_CHOICES,
  canTurnInto,
  deleteBlocks,
  duplicateBlocks,
  moveBlocksBy,
  turnInto,
  type BlockChoice,
  type BlockMenuRequest,
  type PageEditor,
} from "../../editor/index.ts";
import { t } from "../../i18n/index.ts";
import { Menu, type MenuEntry } from "../common/menu.tsx";
import { SLASH_ITEMS } from "./slash-items.ts";

export interface BlockMenuProps {
  request: BlockMenuRequest;
  editor: PageEditor;
  onClose(): void;
}

/** Actions for the selected blocks: turn into, duplicate, move, delete. */
export function BlockMenu(props: BlockMenuProps): JSX.Element {
  const editor = props.editor;
  const view = () => editor.view;
  const run = (command: Command) => () => {
    command(view().state, view().dispatch);
    view().focus();
  };
  const convertible = () => canTurnInto(view().state);

  const entries = (): MenuEntry[] => {
    const turn: MenuEntry[] = TURN_INTO_CHOICES.map((choice: BlockChoice) => {
      const item = SLASH_ITEMS.find((candidate) => candidate.id === choice);
      return {
        kind: "item" as const,
        label: item ? t(item.label) : choice,
        icon: item?.icon,
        disabled: !convertible(),
        onSelect: run(turnInto(choice)),
      };
    });
    return [
      { kind: "heading", label: t("blockMenu.turnInto") },
      ...turn,
      { kind: "separator" },
      { kind: "item", icon: "copy", label: t("blockMenu.duplicate"), onSelect: run(duplicateBlocks) },
      { kind: "item", icon: "arrow-up", label: t("blockMenu.moveUp"), onSelect: run(moveBlocksBy(-1)) },
      { kind: "item", icon: "arrow-down", label: t("blockMenu.moveDown"), onSelect: run(moveBlocksBy(1)) },
      { kind: "separator" },
      { kind: "item", icon: "trash", danger: true, label: t("blockMenu.delete"), onSelect: run(deleteBlocks) },
    ];
  };

  return <Menu label={t("blockMenu.label")} at={{ x: props.request.anchor.left, y: props.request.anchor.bottom + 4 }} entries={entries()} onClose={props.onClose} />;
}
