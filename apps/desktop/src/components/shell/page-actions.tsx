import { Show, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { IconButton } from "../common/button.tsx";

/** Top-bar actions for the page in view: history, a reminder, and saving it as a template. */
export function PageActions(): JSX.Element {
  const app = useApp();
  const id = () => app.tabs.activePageId();
  const isDatabase = () => {
    const current = id();
    return current !== null && app.tree.index().byId.get(current)?.kind === "database";
  };
  return (
    <Show when={id() && !isDatabase()}>
      <IconButton icon="clock" label={t("history.title")} pressed={app.ui.sidePanel() === "history"} onClick={() => app.ui.toggleSidePanel("history")} />
      <IconButton
        icon="bell"
        label={t("reminder.remindMe")}
        onClick={() => {
          const pageId = id();
          if (pageId) app.ui.openDialog({ kind: "reminder", pageId, ...(app.tabs.activeDatabaseId() ? { databaseId: app.tabs.activeDatabaseId() as string } : {}) });
        }}
      />
      <IconButton
        icon="copy"
        label={t("tree.saveAsTemplate")}
        onClick={() => {
          const pageId = id();
          if (pageId) app.ui.openDialog({ kind: "save-template", pageId });
        }}
      />
    </Show>
  );
}
