import { createSignal, type JSX } from "solid-js";
import { clearFind, getFindState, setFindQuery, stepFind, type PageEditor } from "../../editor/index.ts";
import { t } from "../../i18n/index.ts";
import { IconButton } from "../common/button.tsx";

export interface FindBarProps {
  editor: PageEditor;
  /** Changes whenever the editor state changes, so the count stays current. */
  tick: () => number;
  onClose(): void;
}

/** In-page find: type to highlight, Enter for next, Shift+Enter for previous, Escape to close. */
export function FindBar(props: FindBarProps): JSX.Element {
  const [query, setQuery] = createSignal("");
  // Taken once: closing the bar unmounts it, and its handlers still run after that.
  const editor = props.editor;
  const view = () => editor.view;
  const state = () => {
    props.tick();
    return getFindState(view().state);
  };

  const close = () => {
    clearFind(view());
    props.onClose();
    view().focus();
  };

  const summary = () => {
    const current = state();
    if (!query()) return "";
    if (current.matches.length === 0) return t("find.noMatches");
    return t("find.count", { current: current.current + 1, total: current.matches.length });
  };

  return (
    <div class="find-bar" role="search" aria-label={t("find.label")}>
      <input
        type="search"
        aria-label={t("find.label")}
        placeholder={t("find.placeholder")}
        value={query()}
        data-autofocus
        ref={(element) => queueMicrotask(() => (element.focus(), element.select()))}
        onInput={(event) => {
          setQuery(event.currentTarget.value);
          setFindQuery(view(), event.currentTarget.value);
        }}
        onKeyDown={(event) => {
          if (event.key === "Enter") {
            event.preventDefault();
            stepFind(view(), event.shiftKey ? -1 : 1);
          } else if (event.key === "Escape") {
            event.preventDefault();
            event.stopPropagation();
            close();
          }
        }}
      />
      <span class="find-count" aria-live="polite">
        {summary()}
      </span>
      <IconButton icon="arrow-up" label={t("find.previous")} disabled={state().matches.length === 0} onClick={() => stepFind(view(), -1)} />
      <IconButton icon="arrow-down" label={t("find.next")} disabled={state().matches.length === 0} onClick={() => stepFind(view(), 1)} />
      <IconButton icon="close" label={t("common.close")} onClick={close} />
    </div>
  );
}
