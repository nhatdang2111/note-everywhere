import { For, Show, createResource, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button, IconButton } from "./button.tsx";
import { Dialog } from "./dialog.tsx";
import { TextField } from "./form.tsx";
import { EmptyState, ErrorBanner, Spinner } from "./states.tsx";
import { toApiError } from "../../api/errors.ts";

/** Pick what a new page starts from: nothing, or one of the saved page templates. */
export function TemplateGallery(props: { parentId: string | null; onClose(): void }): JSX.Element {
  const app = useApp();
  const parentId = props.parentId;
  const [templates, { refetch }] = createResource(() => app.core.api.templateList({}));
  const [renaming, setRenaming] = createSignal<string | null>(null);

  const use = async (id: string | null) => {
    props.onClose();
    if (id === null) return void (await app.createNode("page", parentId));
    const pageId = await app.guard(() => app.core.api.templateInstantiatePage({ id, parentId }));
    if (!pageId) return;
    if (parentId) app.tree.setExpanded(parentId, true);
    await app.tree.refresh();
    app.openPage(pageId);
  };

  const commitRename = async (id: string, name: string) => {
    setRenaming(null);
    if (name.trim()) await app.guard(() => app.core.api.templateRename({ id, name }));
    await refetch();
  };

  return (
    <Dialog title={t("template.galleryTitle")} wide onClose={props.onClose}>
      <ErrorBanner error={templates.error ? toApiError(templates.error) : null} />
      <Show when={!templates.loading} fallback={<Spinner />}>
        <ul class="gallery">
          <li>
            <button type="button" class="gallery-card" data-autofocus onClick={() => void use(null)}>
              <strong>{t("template.blank")}</strong>
              <span>{t("template.blankHint")}</span>
            </button>
          </li>
          <For each={templates() ?? []}>
            {(template) => (
              <li class="gallery-item">
                <Show
                  when={renaming() === template.id}
                  fallback={
                    <button type="button" class="gallery-card" onClick={() => void use(template.id)}>
                      <strong>{template.name}</strong>
                      <span>{t("template.useHint")}</span>
                    </button>
                  }
                >
                  <input
                    class="view-rename"
                    aria-label={t("template.name")}
                    value={template.name}
                    ref={(element) => queueMicrotask(() => (element.focus(), element.select()))}
                    onKeyDown={(event) => {
                      if (event.key === "Enter") void commitRename(template.id, event.currentTarget.value);
                      else if (event.key === "Escape") (event.stopPropagation(), setRenaming(null));
                    }}
                    onBlur={(event) => renaming() === template.id && void commitRename(template.id, event.currentTarget.value)}
                  />
                </Show>
                <span class="gallery-actions">
                  <IconButton icon="edit" size={14} label={t("template.rename", { name: template.name })} onClick={() => setRenaming(template.id)} />
                  <IconButton icon="trash" size={14} label={t("template.delete", { name: template.name })} onClick={() => void app.guard(() => app.core.api.templateDelete({ id: template.id })).then(() => refetch())} />
                </span>
              </li>
            )}
          </For>
        </ul>
        <Show when={(templates() ?? []).length === 0}>
          <EmptyState icon="page" title={t("template.emptyTitle")}>{t("template.emptyBody")}</EmptyState>
        </Show>
      </Show>
    </Dialog>
  );
}

/** Name a template made from the open page, or from the open row (keeping its values). */
export function SaveTemplateDialog(props: { pageId: string; onClose(): void }): JSX.Element {
  const app = useApp();
  const [name, setName] = createSignal(app.pageTitle(props.pageId));
  const [busy, setBusy] = createSignal(false);
  const databaseId = () => (app.tree.index().byId.has(props.pageId) ? undefined : app.rows.databaseOf(props.pageId));

  const save = async () => {
    if (!name().trim()) return;
    setBusy(true);
    const owner = databaseId();
    const id = await app.guard(() =>
      owner
        ? app.core.api.templateCreateForDatabase({ databaseId: owner, name: name().trim(), fromRowId: props.pageId })
        : app.core.api.templateCreateFromPage({ pageId: props.pageId, name: name().trim() }),
    );
    setBusy(false);
    if (id) {
      app.toasts.push("success", t("template.saved", { name: name().trim() }));
      props.onClose();
    }
  };

  return (
    <Dialog
      title={t(databaseId() ? "template.saveRowTitle" : "template.saveTitle")}
      onClose={props.onClose}
      dismissOnBackdrop={false}
      footer={
        <>
          <Button onClick={props.onClose}>{t("common.cancel")}</Button>
          <Button variant="primary" busy={busy()} disabled={!name().trim()} onClick={() => void save()}>{t("common.save")}</Button>
        </>
      }
    >
      <form onSubmit={(event) => (event.preventDefault(), void save())}>
        <TextField label={t("template.name")} value={name()} onInput={setName} autofocus maxLength={80} hint={t(databaseId() ? "template.saveRowHint" : "template.saveHint")} />
      </form>
    </Dialog>
  );
}
