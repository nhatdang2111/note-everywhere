import { For, Match, Show, Switch, createResource, createSignal, type JSX } from "solid-js";
import { toApiError } from "../../api/errors.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { ConfirmDialog } from "../common/confirm-dialog.tsx";
import { Dialog } from "../common/dialog.tsx";
import { EmptyState, ErrorBanner, Spinner } from "../common/states.tsx";

export type SyncedDialog =
  | { kind: "create"; start: number; end: number }
  | { kind: "insert"; index: number }
  | { kind: "pages"; syncId: string }
  | { kind: "unsync"; syncId: string; path: number[] };

export interface SyncedDialogsProps {
  pageId: string;
  dialog: SyncedDialog;
  /** Wait for local edits to reach the core; false when some could not be saved. */
  flush(): Promise<boolean>;
  onClose(): void;
}

function Pages(props: { syncId: string; onClose(): void }): JSX.Element {
  const app = useApp();
  const [pages] = createResource(() => app.core.api.syncedOccurrences({ syncId: props.syncId }));
  return (
    <Dialog title={t("synced.pagesTitle")} onClose={props.onClose} footer={<Button variant="primary" onClick={props.onClose}>{t("common.done")}</Button>}>
      <ErrorBanner error={pages.error ? toApiError(pages.error) : null} />
      <Show when={!pages.loading} fallback={<Spinner />}>
        <ul class="synced-pages">
          <For each={pages() ?? []}>
            {(page) => (
              <li>
                <Show when={!page.trashed} fallback={<span>{page.title || t("tree.untitled")} <span class="field-hint">{t("synced.inTrash")}</span></span>}>
                  <button type="button" class="link-button" onClick={() => (app.openPage(page.pageId), props.onClose())}>{page.title || t("tree.untitled")}</button>
                </Show>
              </li>
            )}
          </For>
        </ul>
      </Show>
    </Dialog>
  );
}

function InsertExisting(props: { pageId: string; index: number; flush(): Promise<boolean>; onClose(): void }): JSX.Element {
  const app = useApp();
  const [blocks] = createResource(async () => {
    const list = await app.core.api.syncedList();
    return Promise.all(list.map(async (info) => ({ info, pages: await app.core.api.syncedOccurrences({ syncId: info.syncId }).catch(() => []) })));
  });
  const [busy, setBusy] = createSignal(false);

  const insert = async (syncId: string) => {
    setBusy(true);
    const saved = await props.flush();
    const ok = saved && (await app.guard(async () => (await app.core.api.syncedInsert({ pageId: props.pageId, parentPath: [], index: props.index, syncId }), true)));
    setBusy(false);
    if (ok) props.onClose();
  };

  return (
    <Dialog title={t("synced.insertTitle")} onClose={props.onClose}>
      <ErrorBanner error={blocks.error ? toApiError(blocks.error) : null} />
      <Show when={!blocks.loading} fallback={<Spinner />}>
        <Show when={(blocks() ?? []).length > 0} fallback={<EmptyState icon="sync" title={t("synced.noneTitle")}>{t("synced.noneBody")}</EmptyState>}>
          <ul class="synced-pages">
            <For each={blocks() ?? []}>
              {({ info, pages }) => (
                <li>
                  <button type="button" class="gallery-card" disabled={busy()} onClick={() => void insert(info.syncId)}>
                    <strong>{pages[0]?.title || t("tree.untitled")}</strong>
                    <span>{t("synced.count", { count: info.occurrences })}</span>
                  </button>
                </li>
              )}
            </For>
          </ul>
        </Show>
      </Show>
    </Dialog>
  );
}

/** The dialogs behind the synced-block menu items: create, insert an existing one, list the pages, unsync. */
export function SyncedDialogs(props: SyncedDialogsProps): JSX.Element {
  const app = useApp();
  const [busy, setBusy] = createSignal(false);
  const run = async (action: () => Promise<unknown>, done: string) => {
    setBusy(true);
    const saved = await props.flush();
    const ok = saved && (await app.guard(async () => (await action(), true)));
    setBusy(false);
    if (ok) app.toasts.push("success", done);
    props.onClose();
  };
  const dialog = () => props.dialog;
  return (
    <Switch>
      <Match when={dialog().kind === "create"}>
        <ConfirmDialog
          title={t("synced.createTitle")}
          message={t("synced.createBody")}
          confirmLabel={t("synced.create")}
          busy={busy()}
          onCancel={props.onClose}
          onConfirm={() => {
            const { start, end } = dialog() as Extract<SyncedDialog, { kind: "create" }>;
            void run(() => app.core.api.syncedCreate({ pageId: props.pageId, start, end }), t("synced.created"));
          }}
        />
      </Match>
      <Match when={dialog().kind === "unsync"}>
        <ConfirmDialog
          title={t("synced.unsyncTitle")}
          message={t("synced.unsyncBody")}
          confirmLabel={t("synced.unsync")}
          busy={busy()}
          onCancel={props.onClose}
          onConfirm={() => {
            const { path } = dialog() as Extract<SyncedDialog, { kind: "unsync" }>;
            void run(() => app.core.api.syncedUnsync({ pageId: props.pageId, path }), t("synced.unsynced"));
          }}
        />
      </Match>
      <Match when={dialog().kind === "pages"}>
        <Pages syncId={(dialog() as Extract<SyncedDialog, { kind: "pages" }>).syncId} onClose={props.onClose} />
      </Match>
      <Match when={dialog().kind === "insert"}>
        <InsertExisting pageId={props.pageId} index={(dialog() as Extract<SyncedDialog, { kind: "insert" }>).index} flush={props.flush} onClose={props.onClose} />
      </Match>
    </Switch>
  );
}
