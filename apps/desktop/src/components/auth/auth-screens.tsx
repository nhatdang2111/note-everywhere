import { Match, Show, Switch, createSignal, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { createSubmit } from "../../lib/use-submit.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Icon } from "../common/icons.tsx";
import { ErrorBanner } from "../common/states.tsx";
import { RecoverForm, SignInForm, SignUpForm, type AccountFormProps } from "./account-forms.tsx";
import { RecoveryKeyPanel } from "./recovery-key-panel.tsx";

export function AuthFrame(props: { title: string; subtitle?: string; children: JSX.Element }): JSX.Element {
  return (
    <main class="auth-screen">
      <section class="auth-card" aria-labelledby="auth-title">
        <div class="brand" aria-hidden="true">
          <span class="brand-mark">NE</span>
        </div>
        <h1 id="auth-title">{props.title}</h1>
        <Show when={props.subtitle}>
          <p class="auth-subtitle">{props.subtitle}</p>
        </Show>
        {props.children}
      </section>
    </main>
  );
}

type Mode = "signup" | "signin" | "recover";

/** Sign up, sign in and recover share one tab strip. */
export function AccountFlow(props: AccountFormProps & { initial?: Mode }): JSX.Element {
  const [mode, setMode] = createSignal<Mode>(props.initial ?? "signup");
  const tab = (id: Mode, label: string) => (
    <button type="button" role="tab" aria-selected={mode() === id} class="tab-button" onClick={() => setMode(id)}>
      {label}
    </button>
  );
  return (
    <div class="account-flow">
      <div class="tab-strip" role="tablist" aria-label={t("auth.accountTabs")}>
        {tab("signup", t("auth.signUp"))}
        {tab("signin", t("auth.signIn"))}
        {tab("recover", t("auth.recover"))}
      </div>
      <Switch>
        <Match when={mode() === "signup"}>
          <SignUpForm {...props} />
        </Match>
        <Match when={mode() === "signin"}>
          <SignInForm {...props} />
        </Match>
        <Match when={mode() === "recover"}>
          <RecoverForm {...props} />
        </Match>
      </Switch>
    </div>
  );
}

/** First launch: keep notes on this device, or sync through the person's own server. */
export function FirstRun(): JSX.Element {
  const { session } = useApp();
  const [step, setStep] = createSignal<"choose" | "server">("choose");
  const local = createSubmit(() => session.createLocal());

  return (
    <AuthFrame title={t("firstRun.title")} subtitle={step() === "choose" ? t("firstRun.subtitle") : t("firstRun.serverSubtitle")}>
      <Show
        when={step() === "choose"}
        fallback={
          <>
            <AccountFlow />
            <Button class="link-button" variant="ghost" onClick={() => setStep("choose")}>
              {t("firstRun.backToChoice")}
            </Button>
          </>
        }
      >
        <div class="choice-list">
          <button type="button" class="choice" disabled={local.busy()} onClick={() => void local.submit()}>
            <Icon name="lock" size={20} />
            <span class="choice-text">
              <strong>{t("firstRun.local")}</strong>
              <span>{t("firstRun.localHint")}</span>
            </span>
          </button>
          <button type="button" class="choice" onClick={() => setStep("server")}>
            <Icon name="sync" size={20} />
            <span class="choice-text">
              <strong>{t("firstRun.server")}</strong>
              <span>{t("firstRun.serverHint")}</span>
            </span>
          </button>
        </div>
        <ErrorBanner error={local.error()} />
      </Show>
    </AuthFrame>
  );
}

/** The stored sign-in was rejected: ask for the password again, or for the recovery key. */
export function NeedsSignIn(): JSX.Element {
  const { session } = useApp();
  const [recovering, setRecovering] = createSignal(false);
  const prefill = () => ({ serverUrl: session.status()?.serverUrl, username: session.status()?.username });
  return (
    <AuthFrame title={t("needsSignIn.title")} subtitle={t("needsSignIn.subtitle")}>
      <Show when={!recovering()} fallback={<RecoverForm {...prefill()} onBack={() => setRecovering(false)} />}>
        <SignInForm {...prefill()} />
        <Button class="link-button" variant="ghost" onClick={() => setRecovering(true)}>
          {t("needsSignIn.useRecoveryKey")}
        </Button>
      </Show>
    </AuthFrame>
  );
}

/** Shown over everything right after sign-up until the recovery key is confirmed saved. */
export function AccountRecoveryScreen(props: { recoveryKey: string }): JSX.Element {
  const { session } = useApp();
  return (
    <AuthFrame title={t("recovery.accountTitle")}>
      <RecoveryKeyPanel recoveryKey={props.recoveryKey} subject="account" onConfirm={() => session.confirmRecoveryKey()} />
    </AuthFrame>
  );
}
