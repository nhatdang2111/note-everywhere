import { For, Show, createEffect, createResource, createSignal, on, type JSX } from "solid-js";
import type { PageVersion } from "../../api/contract.ts";
import { toApiError } from "../../api/errors.ts";
import { t } from "../../i18n/index.ts";
import { formatDateTime } from "../../lib/format.ts";
import { useApp } from "../../state/app.tsx";
import { Button, IconButton } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";
import { EmptyState, ErrorBanner, Spinner } from "../common/states.tsx";
import { PreviewBlocks } from "./preview-blocks.tsx";

const shortAuthor = (author: string) => author.slice(-4);

function when(version: PageVersion): string {
  if (version.startedAt === null) return t("history.noTime");
  return version.endedAt && version.endedAt - version.startedAt > 60_000 ? `${formatDateTime(version.startedAt)} – ${formatDateTime(version.endedAt)}` : formatDateTime(version.startedAt);
}

/** Versions of the open page, newest first; select one to read it, restore it after confirming. */
export function HistoryPanel(): JSX.Element {
  const app = useApp();
  const pageId = () => {
    const id = app.tabs.activePageId();
    return id && app.tree.index().byId.get(id)?.kind !== "database" ? id : null;
  };
  const [selected, setSelected] = createSignal<string | null>(null);
  const [confirming, setConfirming] = createSignal(false);
  const [busy, setBusy] = createSignal(false);

  const [history, { refetch }] = createResource(pageId, (id) => app.core.api.pageHistory({ id }));
  const [preview] = createResource(
    () => (pageId() && selected() && history()?.previewable ? { id: pageId() as string, versionId: selected() as string } : null),
    (args) => app.core.api.pageVersionPreview(args),
  );
  createEffect(on(pageId, () => setSelected(null)));

  const restore = async () => {
    const id = pageId();
    const versionId = selected();
    if (!id || !versionId) return;
    setBusy(true);
    const ok = await app.guard(async () => (await app.core.api.pageVersionRestore({ id, versionId }), true));
    setBusy(false);
    setConfirming(false);
    if (ok) {
      app.toasts.push("success", t("history.restored"));
      setSelected(null);
      await refetch();
    }
  };

  return (
    <aside class="side-panel" aria-label={t("history.title")}>
      <header class="side-panel-head">
        <h2>{t("history.title")}</h2>
        <IconButton icon="sync" size={14} label={t("history.refresh")} onClick={() => void refetch()} />
        <IconButton icon="close" size={14} label={t("common.close")} onClick={() => app.ui.closeSidePanel()} />
      </header>
      <Show when={pageId()} fallback={<EmptyState icon="clock" title={t("history.noPageTitle")}>{t("history.noPageBody")}</EmptyState>}>
        <ErrorBanner error={history.error ? toApiError(history.error) : null} />
        <Show when={history()} fallback={<Show when={history.loading}><Spinner /></Show>}>
          {(data) => (
            <>
              <Show when={data().trimmed}>
                <p class="notice" role="note">{t("history.trimmed")}</p>
              </Show>
              <Show when={!data().trimmed && data().oldestAt}>
                <p class="field-hint">{t("history.since", { when: formatDateTime(data().oldestAt ?? 0) })}</p>
              </Show>
              <Show when={data().versions.length > 0} fallback={<EmptyState icon="clock" title={t("history.emptyTitle")}>{t("history.emptyBody")}</EmptyState>}>
                <ul class="version-list" aria-label={t("history.versions")}>
                  <For each={data().versions}>
                    {(version) => (
                      <li>
                        <button type="button" class="version" aria-pressed={selected() === version.id} disabled={!data().previewable} onClick={() => setSelected(version.id)}>
                          <span class="version-time">{when(version)}</span>
                          <span class="version-meta">{t("history.meta", { changes: version.changes, device: shortAuthor(version.author) })}</span>
                        </button>
                      </li>
                    )}
                  </For>
                </ul>
              </Show>
              <Show when={selected()}>
                <section class="version-preview" aria-label={t("history.preview")}>
                  <h3>{t("history.preview")}</h3>
                  <ErrorBanner error={preview.error ? toApiError(preview.error) : null} />
                  <Show when={preview()} fallback={<Show when={preview.loading}><Spinner /></Show>}>{(blocks) => <PreviewBlocks blocks={blocks()} />}</Show>
                  <Button variant="primary" disabled={!preview()} onClick={() => setConfirming(true)}>{t("history.restore")}</Button>
                </section>
              </Show>
            </>
          )}
        </Show>
      </Show>
      <Show when={confirming()}>
        <ConfirmDialog
          title={t("history.restoreTitle")}
          message={t("history.restoreBody")}
          confirmLabel={t("history.restore")}
          busy={busy()}
          onCancel={() => setConfirming(false)}
          onConfirm={() => void restore()}
        />
      </Show>
    </aside>
  );
}
