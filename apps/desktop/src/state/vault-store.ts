import { createMemo, createSignal } from "solid-js";
import type { Core } from "../api/index.ts";
import type {
  ApiError,
  GeneratePasswordArgs,
  GeneratedPassword,
  VaultCsvFormat,
  VaultImportReport,
  VaultItemInput,
  VaultItemSummary,
  VaultSecretField,
  VaultState,
} from "../api/contract.ts";
import { apiError, toApiError } from "../api/errors.ts";

export interface ReauthRequest {
  /** Set after a wrong password so the dialog can say so. */
  message: string | null;
  /** The entered password is being checked. */
  busy: boolean;
  submit(password: string): void;
  cancel(): void;
}

export interface ClipboardCountdown {
  field: VaultSecretField | "username";
  remaining: number;
}

export interface VaultStore {
  state: () => VaultState | null;
  items: () => VaultItemSummary[];
  /** Items after the tag filter; the text query is applied by the core. */
  visibleItems: () => VaultItemSummary[];
  tags: () => string[];
  query: () => string;
  setQuery(query: string): void;
  activeTag: () => string | null;
  setActiveTag(tag: string | null): void;
  loadError: () => ApiError | null;
  reauth: () => ReauthRequest | null;
  clipboard: () => ClipboardCountdown | null;
  pendingRecoveryKey: () => string | null;
  confirmRecoveryKey(): void;
  refreshState(): Promise<void>;
  refreshItems(): Promise<void>;
  create(password: string): Promise<{ sameAsAccountPassword: boolean }>;
  unlock(password: string): Promise<void>;
  lock(): Promise<void>;
  recover(recoveryKey: string, newPassword: string): Promise<void>;
  changePassword(currentPassword: string, newPassword: string): Promise<void>;
  save(item: VaultItemInput): Promise<VaultItemSummary>;
  remove(id: string): Promise<void>;
  /** Resolves to the secret; asks for the vault password first when the core requires it. */
  reveal(id: string, field: VaultSecretField): Promise<string>;
  copy(id: string, field: VaultSecretField | "username"): Promise<void>;
  generate(args: GeneratePasswordArgs): Promise<GeneratedPassword>;
  importCsv(format: VaultCsvFormat): Promise<VaultImportReport | null>;
  dispose(): void;
}

const CANCELLED = apiError("cancelled", "Cancelled.");
const SEARCH_DELAY_MS = 200;

export function createVaultStore(core: Core): VaultStore {
  const [state, setState] = createSignal<VaultState | null>(null);
  const [items, setItems] = createSignal<VaultItemSummary[]>([]);
  const [query, setQueryValue] = createSignal("");
  const [activeTag, setActiveTag] = createSignal<string | null>(null);
  const [loadError, setLoadError] = createSignal<ApiError | null>(null);
  const [reauth, setReauth] = createSignal<ReauthRequest | null>(null);
  const [clipboard, setClipboard] = createSignal<ClipboardCountdown | null>(null);
  const [pendingRecoveryKey, setPendingRecoveryKey] = createSignal<string | null>(null);

  let countdown: ReturnType<typeof setInterval> | undefined;
  let searchTimer: ReturnType<typeof setTimeout> | undefined;
  let listVersion = 0;

  const tags = createMemo(() => [...new Set(items().flatMap((item) => item.tags))].sort((a, b) => a.localeCompare(b)));
  const visibleItems = createMemo(() => {
    const tag = activeTag();
    return tag ? items().filter((item) => item.tags.includes(tag)) : items();
  });

  /** Everything the vault showed goes away the moment it is not unlocked. */
  function forget(): void {
    listVersion += 1;
    setItems([]);
    setQueryValue("");
    setActiveTag(null);
    setLoadError(null);
    reauth()?.cancel();
  }

  function applyState(next: VaultState): void {
    setState(next);
    if (next !== "unlocked") forget();
  }

  void core.events.on("vault:state", ({ state: next }) => {
    applyState(next);
    if (next === "unlocked") void refreshItems();
  });

  async function refreshState(): Promise<void> {
    try {
      applyState(await core.api.vaultState());
      if (state() === "unlocked") await refreshItems();
    } catch (error) {
      setLoadError(toApiError(error));
    }
  }

  async function refreshItems(): Promise<void> {
    const version = (listVersion += 1);
    try {
      const list = await core.api.vaultList({ query: query().trim() || undefined });
      if (version === listVersion && state() === "unlocked") {
        setItems(list);
        setLoadError(null);
      }
    } catch (error) {
      if (version === listVersion) setLoadError(toApiError(error));
    }
  }

  /** Run a secret-bearing call; on `reauth_required` ask for the password until it is accepted or cancelled. */
  async function withReauth<T>(run: (password?: string) => Promise<T>): Promise<T> {
    try {
      return await run();
    } catch (first) {
      if (toApiError(first).code !== "reauth_required") throw first;
    }
    let message: string | null = null;
    // One dialog for the whole exchange: it stays open while a password is checked.
    const close = () => setReauth(null);
    try {
      for (;;) {
        const password = await new Promise<string | null>((resolve) => {
          setReauth({ message, busy: false, submit: (value) => resolve(value), cancel: () => resolve(null) });
        });
        if (password === null) throw CANCELLED;
        setReauth({ message, busy: true, submit: () => undefined, cancel: () => undefined });
        try {
          return await run(password);
        } catch (error) {
          const failure = toApiError(error);
          if (failure.code !== "wrong_password") throw error;
          message = failure.message;
        }
      }
    } finally {
      close();
    }
  }

  function startCountdown(field: ClipboardCountdown["field"], seconds: number): void {
    clearInterval(countdown);
    setClipboard({ field, remaining: seconds });
    countdown = setInterval(() => {
      const current = clipboard();
      if (!current || current.remaining <= 1) {
        clearInterval(countdown);
        setClipboard(null);
      } else {
        setClipboard({ ...current, remaining: current.remaining - 1 });
      }
    }, 1000);
  }

  return {
    state,
    items,
    visibleItems,
    tags,
    query,
    setQuery(next) {
      setQueryValue(next);
      clearTimeout(searchTimer);
      searchTimer = setTimeout(() => void refreshItems(), SEARCH_DELAY_MS);
    },
    activeTag,
    setActiveTag,
    loadError,
    reauth,
    clipboard,
    pendingRecoveryKey,
    confirmRecoveryKey: () => setPendingRecoveryKey(null),
    refreshState,
    refreshItems,
    async create(password) {
      const { recoveryKey, sameAsAccountPassword } = await core.api.vaultCreate({ password });
      setPendingRecoveryKey(recoveryKey);
      await refreshState();
      return { sameAsAccountPassword };
    },
    async unlock(password) {
      await core.api.vaultUnlock({ password });
      await refreshState();
    },
    async lock() {
      await core.api.vaultLock();
      applyState("locked");
    },
    async recover(recoveryKey, newPassword) {
      await core.api.vaultRecover({ recoveryKey, newPassword });
      await refreshState();
    },
    changePassword: (currentPassword, newPassword) => core.api.vaultChangePassword({ currentPassword, newPassword }),
    async save(item) {
      const saved = await core.api.vaultSave({ item });
      await refreshItems();
      return saved;
    },
    async remove(id) {
      await core.api.vaultDelete({ id });
      await refreshItems();
    },
    reveal: (id, field) => withReauth((password) => core.api.vaultReveal({ id, field, password })),
    async copy(id, field) {
      const { clearAfterSeconds } = await withReauth((password) => core.api.vaultCopy({ id, field, password }));
      startCountdown(field, clearAfterSeconds);
    },
    generate: (args) => core.api.vaultGeneratePassword(args),
    async importCsv(format) {
      const report = await core.api.vaultImportCsv({ format });
      await refreshItems();
      return report;
    },
    dispose() {
      clearInterval(countdown);
      clearTimeout(searchTimer);
    },
  };
}
