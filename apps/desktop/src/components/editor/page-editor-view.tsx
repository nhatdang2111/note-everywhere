import { Show, createEffect, createSignal, on, onCleanup, onMount, type JSX } from "solid-js";
import { Selection } from "prosemirror-state";
import type { ApiError } from "../../api/contract.ts";
import { toApiError } from "../../api/errors.ts";
import {
  attachmentSrc,
  createPageEditor,
  isSafeHref,
  linkAt,
  parseAttachmentSrc,
  type BlockMenuRequest,
  type EditorHost,
  type PageEditor,
  type SuggestionState,
} from "../../editor/index.ts";
import { createPageDoc } from "../../editor/binding.ts";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { ErrorBanner, Spinner } from "../common/states.tsx";
import { BacklinksPanel } from "./backlinks-panel.tsx";
import { BlockMenu } from "./block-menu.tsx";
import { FindBar } from "./find-bar.tsx";
import { FormatToolbar } from "./format-toolbar.tsx";
import { PagePickerDialog } from "./page-picker-dialog.tsx";
import { SuggestionMenu, type KeyHandler } from "./suggestion-menu.tsx";

type PickedPage = { id: string; title: string };

function editorLabels() {
  return {
    toggleBlock: t("editor.toggleBlock"),
    todoCheckbox: t("editor.todoCheckbox"),
    codeLanguage: t("editor.codeLanguage"),
    blockHandle: t("editor.blockHandle"),
    imageMissing: t("editor.imageMissing"),
    chooseAPage: t("editor.chooseAPage"),
    subpage: t("editor.subpage"),
    linkedPage: t("editor.linkedPage"),
    pageInTrash: t("editor.pageInTrash"),
    pageMissing: t("editor.pageMissing"),
    editorLabel: t("editor.label"),
  };
}

/** One open page: loads the snapshot, runs the editor, and keeps core and editor in step. */
export function PageEditorView(props: { pageId: string }): JSX.Element {
  const app = useApp();
  const id = props.pageId;
  let mount!: HTMLDivElement;
  let titleInput!: HTMLInputElement;

  const [phase, setPhase] = createSignal<"loading" | "ready" | "error">("loading");
  const [loadError, setLoadError] = createSignal<ApiError | null>(null);
  const [saveError, setSaveError] = createSignal<ApiError | null>(null);
  const [editor, setEditor] = createSignal<PageEditor | null>(null);
  const [tick, setTick] = createSignal(0);
  const [suggestion, setSuggestion] = createSignal<SuggestionState | null>(null);
  const [blockMenu, setBlockMenu] = createSignal<BlockMenuRequest | null>(null);
  const [findOpen, setFindOpen] = createSignal(false);
  const [linkRequest, setLinkRequest] = createSignal(0);
  const [picker, setPicker] = createSignal<{ resolve(page: PickedPage | null): void } | null>(null);
  const [draftTitle, setDraftTitle] = createSignal<string | null>(null);
  let suggestionKeys: KeyHandler | null = null;
  let titleTimer: ReturnType<typeof setTimeout> | undefined;

  // Local edits go to the core in order; a failed send stays queued and is retried.
  const outbox: Uint8Array[] = [];
  let chain: Promise<void> = Promise.resolve();
  const drain = async () => {
    while (outbox.length > 0) {
      try {
        await app.core.api.pageApplyUpdate({ id, update: outbox[0] as Uint8Array });
        outbox.shift();
        setSaveError(null);
      } catch (error) {
        setSaveError(toApiError(error));
        return;
      }
    }
  };
  const queueUpdate = (update: Uint8Array) => {
    outbox.push(update);
    chain = chain.then(drain);
  };
  const retrySave = () => void (chain = chain.then(drain));

  const host: EditorHost = {
    labels: editorLabels(),
    resolvePage: (pageId) => app.pageInfo(pageId),
    openPage: (pageId, inNewTab) => app.openPage(pageId, inNewTab),
    openExternal: (href) => openExternal(href),
    resolveImage: (src) => {
      const hash = parseAttachmentSrc(src);
      return hash ? app.core.api.attachmentUrl({ hash }) : src;
    },
    addImage: async (file) => {
      const bytes = new Uint8Array(await file.arrayBuffer());
      const added = await app.core.api.attachmentAddBytes({ bytes, mime: file.type || "application/octet-stream", fileName: file.name });
      return attachmentSrc(added.hash);
    },
    onUpdate: () => setTick((n) => n + 1),
    onSuggest: setSuggestion,
    onSuggestKey: (event) => suggestionKeys?.(event) ?? false,
    onBlockMenu: setBlockMenu,
    onLinkShortcut: () => {
      const view = editor()?.view;
      if (!view || (view.state.selection.empty && !linkAt(view.state))) return false;
      setLinkRequest((n) => n + 1);
      return true;
    },
    onError: (message) => void app.toasts.push("error", message),
  };

  const openExternal = (href: string) => {
    if (isSafeHref(href)) window.open(href, "_blank", "noopener,noreferrer");
  };

  onMount(() => {
    let disposed = false;
    let unlisten: (() => void) | undefined;
    let instance: PageEditor | undefined;

    void (async () => {
      try {
        const { snapshot } = await app.core.api.pageOpen({ id });
        if (disposed) return;
        const doc = createPageDoc();
        if (snapshot.byteLength > 0) doc.import(snapshot);
        instance = createPageEditor(mount, { doc, host, onLocalUpdate: queueUpdate });
        setEditor(instance);
        app.ui.setActiveEditor(instance);
        setPhase("ready");
        const stop = await app.core.events.on("page:remote-update", ({ pageId, update }) => {
          if (pageId === id) instance?.importRemote(update);
        });
        if (disposed) stop();
        else unlisten = stop;
        if (app.tree.title(id) === "") titleInput.focus();
        else instance.view.focus();
      } catch (error) {
        if (disposed) return;
        setLoadError(toApiError(error));
        setPhase("error");
      }
    })();

    onCleanup(() => {
      disposed = true;
      unlisten?.();
      clearTimeout(titleTimer);
      if (instance && app.ui.activeEditor() === instance) app.ui.setActiveEditor(null);
      instance?.destroy();
      // Let queued edits reach the core before telling it the page is closed.
      void chain.then(() => app.core.api.pageClose({ id })).catch(() => undefined);
    });
  });

  // Titles and states of linked pages follow the tree.
  createEffect(
    on(
      () => [app.tree.nodes(), app.tree.trash()],
      () => editor()?.refreshLinks(),
      { defer: true },
    ),
  );
  createEffect(on(app.ui.findRequest, () => editor() && setFindOpen(true), { defer: true }));

  const commitTitle = (value: string) => {
    const title = value.trim();
    if (title !== app.tree.title(id)) void app.guard(() => app.tree.rename(id, title));
  };

  const requestPage = () => new Promise<PickedPage | null>((resolve) => setPicker({ resolve }));
  const closePicker = (page: PickedPage | null) => {
    picker()?.resolve(page);
    setPicker(null);
    editor()?.view.focus();
  };

  return (
    <article class="page" aria-label={app.tree.title(id) || t("tree.untitled")}>
      <Show when={findOpen() && editor()}>{(current) => <FindBar editor={current()} tick={tick} onClose={() => setFindOpen(false)} />}</Show>
      <div class="page-scroll">
        <div class="page-column">
          <input
            ref={titleInput}
            class="page-title"
            value={draftTitle() ?? app.tree.title(id)}
            placeholder={t("page.untitled")}
            aria-label={t("page.titleLabel")}
            maxLength={300}
            onInput={(event) => {
              const value = event.currentTarget.value;
              setDraftTitle(value);
              clearTimeout(titleTimer);
              titleTimer = setTimeout(() => commitTitle(value), 500);
            }}
            onBlur={(event) => {
              clearTimeout(titleTimer);
              commitTitle(event.currentTarget.value);
              setDraftTitle(null);
            }}
            onKeyDown={(event) => {
              if (event.key !== "Enter") return;
              event.preventDefault();
              const view = editor()?.view;
              if (!view) return;
              view.dispatch(view.state.tr.setSelection(Selection.atStart(view.state.doc)));
              view.focus();
            }}
          />
          <ErrorBanner error={saveError() ? { code: "save_failed", message: t("page.saveFailed", { reason: saveError()?.message ?? "" }) } : null}>
            <Button onClick={retrySave}>{t("common.retry")}</Button>
          </ErrorBanner>
          <Show when={phase() === "loading"}>
            <Spinner />
          </Show>
          <Show when={phase() === "error"}>
            <ErrorBanner error={loadError()} />
          </Show>
          <div ref={mount} class="page-editor" hidden={phase() !== "ready"} />
          <Show when={phase() === "ready"}>
            <BacklinksPanel pageId={id} />
          </Show>
        </div>
      </div>

      <Show when={editor()}>
        {(current) => (
          <>
            <FormatToolbar editor={current()} tick={tick} linkRequest={linkRequest} onOpenExternal={openExternal} />
            <Show when={suggestion()}>
              {(state) => (
                <SuggestionMenu
                  state={state()}
                  editor={current()}
                  pageId={id}
                  requestPage={requestPage}
                  registerKeys={(handler) => void (suggestionKeys = handler)}
                />
              )}
            </Show>
            <Show when={blockMenu()}>
              {(request) => <BlockMenu request={request()} editor={current()} onClose={() => setBlockMenu(null)} />}
            </Show>
          </>
        )}
      </Show>
      <Show when={picker()}>
        <PagePickerDialog excludeId={id} onPick={closePicker} onCancel={() => closePicker(null)} />
      </Show>
    </article>
  );
}
