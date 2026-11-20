import { Match, Show, Switch, createResource, type JSX } from "solid-js";
import type { Core } from "./api/index.ts";
import { toApiError } from "./api/errors.ts";
import { AccountRecoveryScreen, AuthFrame, FirstRun, NeedsSignIn } from "./components/auth/auth-screens.tsx";
import { Button } from "./components/common/button.tsx";
import { ToastHost } from "./components/common/toast-host.tsx";
import { ErrorBanner, Spinner } from "./components/common/states.tsx";
import { Workspace } from "./components/shell/workspace.tsx";
import { t } from "./i18n/index.ts";
import { AppProvider, useApp } from "./state/app.tsx";

/** Chooses the screen from the session state; the recovery key screen sits above all of them. */
export function Root(): JSX.Element {
  const { session, core } = useApp();
  return (
    <>
      <Show when={core.preview}>
        <div class="preview-banner" role="note">
          {t("shell.previewBanner")}
        </div>
      </Show>
      <Switch fallback={<AuthFrame title={t("app.name")}>{<Spinner />}</AuthFrame>}>
        <Match when={session.pendingRecoveryKey()}>{(key) => <AccountRecoveryScreen recoveryKey={key()} />}</Match>
        <Match when={session.loadError()}>
          <AuthFrame title={t("app.loadErrorTitle")}>
            <ErrorBanner error={session.loadError()} />
            <Button variant="primary" onClick={() => void session.refresh()}>
              {t("common.retry")}
            </Button>
          </AuthFrame>
        </Match>
        <Match when={session.status()?.state === "empty"}>
          <FirstRun />
          <ToastHost />
        </Match>
        <Match when={session.status()?.state === "needs_sign_in"}>
          <NeedsSignIn />
        </Match>
        <Match when={session.status()?.state === "local" || session.status()?.state === "signed_in"}>
          <Workspace />
        </Match>
      </Switch>
    </>
  );
}

/** The whole UI for one core. `core` is the real Tauri bridge or, in tests and preview, the in-memory one. */
export function App(props: { core: Core }): JSX.Element {
  return (
    <AppProvider core={props.core}>
      <Root />
    </AppProvider>
  );
}

/** Loads the core, then renders the app; shows why when no core can be reached. */
export function Bootstrap(props: { load(): Promise<Core> }): JSX.Element {
  const [core] = createResource(props.load);
  return (
    <Show
      when={!core.error}
      fallback={
        <AuthFrame title={t("app.loadErrorTitle")}>
          <ErrorBanner error={toApiError(core.error)} />
        </AuthFrame>
      }
    >
      <Show when={core()} fallback={<AuthFrame title={t("app.name")}>{<Spinner />}</AuthFrame>}>
        {(loaded) => <App core={loaded()} />}
      </Show>
    </Show>
  );
}
