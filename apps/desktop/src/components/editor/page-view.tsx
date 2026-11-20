import { Show, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { EmptyState, Spinner } from "../common/states.tsx";
import { PageEditorView } from "./page-editor-view.tsx";

/** Opens a page, or explains why it cannot be opened. */
export function PageView(props: { pageId: string }): JSX.Element {
  const app = useApp();
  const state = () => app.tree.pageState(props.pageId);
  return (
    <Show when={app.tree.loaded()} fallback={<Spinner />}>
      <Show
        when={state() === "ok"}
        fallback={
          <EmptyState
            icon={state() === "trashed" ? "trash" : "alert"}
            title={state() === "trashed" ? t("page.inTrashTitle") : t("page.missingTitle")}
            action={
              state() === "trashed" ? (
                <Button variant="primary" onClick={() => void app.guard(() => app.tree.restore(props.pageId))}>
                  {t("trash.restore")}
                </Button>
              ) : undefined
            }
          >
            {state() === "trashed" ? t("page.inTrashBody") : t("page.missingBody")}
          </EmptyState>
        }
      >
        <PageEditorView pageId={props.pageId} />
      </Show>
    </Show>
  );
}
