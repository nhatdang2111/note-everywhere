import type { JSX } from "solid-js";
import { t } from "../../i18n/index.ts";

/** Inline rename box: Enter or blur saves, Escape cancels. */
export function RenameInput(props: { initial: string; onCommit(title: string): void; onCancel(): void }): JSX.Element {
  let finished = false;
  const finish = (commit: boolean, value: string) => {
    if (finished) return;
    finished = true;
    if (commit) props.onCommit(value.trim());
    else props.onCancel();
  };
  return (
    <input
      class="tree-rename"
      value={props.initial}
      aria-label={t("tree.renameLabel")}
      ref={(element) =>
        queueMicrotask(() => {
          element.focus();
          element.select();
        })
      }
      onClick={(event) => event.stopPropagation()}
      onKeyDown={(event) => {
        event.stopPropagation();
        if (event.key === "Enter") finish(true, event.currentTarget.value);
        else if (event.key === "Escape") finish(false, "");
      }}
      onBlur={(event) => finish(true, event.currentTarget.value)}
    />
  );
}
