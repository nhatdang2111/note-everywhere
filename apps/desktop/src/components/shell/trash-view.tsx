import { For, Show, createSignal, onMount, type JSX } from "solid-js";
import type { TrashEntry } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { formatRelative } from "../../lib/format.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";
import { Icon } from "../common/icons.tsx";
import { EmptyState, ErrorBanner } from "../common/states.tsx";

type Pending = { kind: "one"; entry: TrashEntry } | { kind: "all" };

export function TrashView(): JSX.Element {
  const app = useApp();
  const { tree } = app;
  const [pending, setPending] = createSignal<Pending | null>(null);
  const [busy, setBusy] = createSignal(false);

  onMount(() => void tree.refreshTrash());

  const confirm = async () => {
    const action = pending();
    if (!action) return;
    setBusy(true);
    await app.guard(() => (action.kind === "all" ? tree.emptyTrash() : tree.deletePermanently(action.entry.id)));
    setBusy(false);
    setPending(null);
  };

  return (
    <section class="page-surface" aria-labelledby="trash-title">
      <header class="view-header">
        <h1 id="trash-title">{t("trash.title")}</h1>
        <Button variant="danger" icon="trash" disabled={tree.trash().length === 0} onClick={() => setPending({ kind: "all" })}>
          {t("trash.empty")}
        </Button>
      </header>
      <p class="view-intro">{t("trash.intro")}</p>
      <ErrorBanner error={tree.error()} />
      <Show
        when={tree.trash().length > 0}
        fallback={
          <EmptyState icon="trash" title={t("trash.emptyTitle")}>
            {t("trash.emptyBody")}
          </EmptyState>
        }
      >
        <ul class="trash-list">
          <For each={tree.trash()}>
            {(entry) => (
              <li class="trash-item">
                <Icon name={entry.kind === "folder" ? "folder" : entry.kind === "database" ? "database" : "page"} />
                <span class="trash-title">{entry.title || t("tree.untitled")}</span>
                <span class="trash-when">{t("trash.deleted", { when: formatRelative(entry.trashedAt) })}</span>
                <Button icon="arrow-left" onClick={() => void app.guard(() => tree.restore(entry.id))}>
                  {t("trash.restore")}
                </Button>
                <Button variant="danger" onClick={() => setPending({ kind: "one", entry })}>
                  {t("trash.deleteForever")}
                </Button>
              </li>
            )}
          </For>
        </ul>
      </Show>
      <Show when={pending()}>
        {(action) => (
          <ConfirmDialog
            title={action().kind === "all" ? t("trash.emptyConfirmTitle") : t("trash.deleteConfirmTitle")}
            message={
              action().kind === "all"
                ? t("trash.emptyConfirmBody")
                : t("trash.deleteConfirmBody", { title: (action() as { entry: TrashEntry }).entry.title || t("tree.untitled") })
            }
            confirmLabel={action().kind === "all" ? t("trash.empty") : t("trash.deleteForever")}
            danger
            busy={busy()}
            onConfirm={() => void confirm()}
            onCancel={() => setPending(null)}
          />
        )}
      </Show>
    </section>
  );
}
