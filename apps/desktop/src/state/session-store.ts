import { createSignal } from "solid-js";
import type { Core } from "../api/index.ts";
import type { ApiError, RecoverAccountArgs, SessionStatus, SignInArgs, SignUpArgs } from "../api/contract.ts";
import { toApiError } from "../api/errors.ts";

export interface SessionStore {
  /** Null until the first read finishes. */
  status: () => SessionStatus | null;
  loadError: () => ApiError | null;
  /**
   * The account recovery key from sign-up, held until the person confirms it
   * was saved. While set, the app shows the recovery screen over everything.
   */
  pendingRecoveryKey: () => string | null;
  confirmRecoveryKey(): void;
  refresh(): Promise<void>;
  createLocal(): Promise<void>;
  signUp(args: SignUpArgs): Promise<void>;
  signIn(args: SignInArgs): Promise<void>;
  recover(args: RecoverAccountArgs): Promise<void>;
  signOut(): Promise<void>;
}

export function createSessionStore(core: Core): SessionStore {
  const [status, setStatus] = createSignal<SessionStatus | null>(null);
  const [loadError, setLoadError] = createSignal<ApiError | null>(null);
  const [pendingRecoveryKey, setPendingRecoveryKey] = createSignal<string | null>(null);

  async function refresh(): Promise<void> {
    try {
      setStatus(await core.api.sessionStatus());
      setLoadError(null);
    } catch (error) {
      setLoadError(toApiError(error));
    }
  }

  void core.events.on("session:changed", (next) => setStatus(next));

  return {
    status,
    loadError,
    pendingRecoveryKey,
    confirmRecoveryKey: () => setPendingRecoveryKey(null),
    refresh,
    async createLocal() {
      await core.api.workspaceCreateLocal();
      await refresh();
    },
    async signUp(args) {
      const { recoveryKey } = await core.api.signUp(args);
      setPendingRecoveryKey(recoveryKey);
      await refresh();
    },
    async signIn(args) {
      await core.api.signIn(args);
      await refresh();
    },
    async recover(args) {
      await core.api.recoverAccount(args);
      await refresh();
    },
    async signOut() {
      await core.api.signOut();
      await refresh();
    },
  };
}
