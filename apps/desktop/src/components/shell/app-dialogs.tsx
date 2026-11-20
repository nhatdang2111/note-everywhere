import { Match, Show, Switch, type JSX } from "solid-js";
import { useApp } from "../../state/app.tsx";
import { SaveTemplateDialog, TemplateGallery } from "../common/template-dialogs.tsx";
import { ReminderDialog } from "../reminders/reminder-dialog.tsx";
import { ImportStructuredDialog } from "./import-structured-dialog.tsx";

/** The dialog any part of the app asked the workspace to show. */
export function AppDialogs(): JSX.Element {
  const { ui } = useApp();
  // Keyed: each dialog gets the request as a plain value, which stays valid while it closes itself.
  return (
    <Show when={ui.dialog()} keyed>
      {(request) => (
        <Switch>
          <Match when={request.kind === "import-structured" && request}>{(open) => <ImportStructuredDialog parentId={open().parentId} onClose={ui.closeDialog} />}</Match>
          <Match when={request.kind === "template-gallery" && request}>{(open) => <TemplateGallery parentId={open().parentId} onClose={ui.closeDialog} />}</Match>
          <Match when={request.kind === "save-template" && request}>{(open) => <SaveTemplateDialog pageId={open().pageId} onClose={ui.closeDialog} />}</Match>
          <Match when={request.kind === "reminder" && request}>
            {(open) => <ReminderDialog pageId={open().pageId} databaseId={open().databaseId} propertyId={open().propertyId} onClose={ui.closeDialog} />}
          </Match>
        </Switch>
      )}
    </Show>
  );
}
