import type {
  AddedAttachment,
  CoreApi,
  DeviceSession,
  SessionState,
  SessionStatus,
  Settings,
  SyncStatus,
  VoiceModel,
  VoiceStatus,
} from "./contract.ts";
import { apiError } from "./errors.ts";
import { MEMORY_SETUP_CODE, MIN_PASSWORD_LENGTH, randomId, randomRecoveryKey, type Bus, type MemoryOptions } from "./memory-bus.ts";

type AccountMethods = Pick<
  CoreApi,
  | "appVersion"
  | "sessionStatus"
  | "workspaceCreateLocal"
  | "signUp"
  | "signIn"
  | "recoverAccount"
  | "signOut"
  | "changePassword"
  | "sessionsList"
  | "sessionsRevoke"
  | "syncStatus"
  | "syncNow"
  | "voiceStatus"
  | "voiceDownloadModel"
  | "voiceDeleteModel"
  | "voiceSelectModel"
  | "voiceStart"
  | "voiceStop"
  | "voiceCancel"
  | "settingsGet"
  | "settingsSet"
>;

export interface Account extends AccountMethods {
  /** Called whenever local data changed; counts as an update waiting to sync. */
  markDirty(): void;
  /** Password of the signed-in account, for the vault's "same password" check. */
  accountPassword(): string | null;
  settings(): Settings;
  control: AccountControl;
}

/** Test and preview levers that have no counterpart in the real contract. */
export interface AccountControl {
  expireSession(): void;
  setOffline(offline: boolean): void;
  flushSync(): void;
  setNextTranscript(text: string): void;
  pressHotkey(pressed: boolean): void;
}

const MODELS: VoiceModel[] = [
  { name: "small-q5", byteLength: 190_000_000, installed: false },
  { name: "large-v3-turbo-q5", byteLength: 574_000_000, installed: false },
];

function requireUrl(url: string): void {
  if (!/^https?:\/\/[^\s/]+/i.test(url)) throw apiError("bad_server_url", "Enter the full server address, starting with https://.");
}

function requirePassword(password: string): void {
  if (password.length < MIN_PASSWORD_LENGTH) {
    throw apiError("weak_password", `Use at least ${MIN_PASSWORD_LENGTH} characters for the password.`);
  }
}

/** Filled in once the pieces that need the account exist. */
export interface AccountHooks {
  /** Store the recording of a hold-to-talk session when the setting asks for it. */
  recordAudio?: () => Promise<AddedAttachment>;
}

export function createAccount(bus: Bus, options: MemoryOptions, hooks: AccountHooks = {}): Account {
  let session: SessionStatus = { state: "empty" };
  const users = new Map<string, { password: string; recoveryKey: string }>();
  let currentPassword: string | null = null;
  const devices: DeviceSession[] = [
    { id: randomId("dev"), deviceLabel: "Studio desktop", createdAt: Date.now() - 9e8, lastSeenAt: Date.now() - 3.6e6, current: false },
    { id: randomId("dev"), deviceLabel: "Travel laptop", createdAt: Date.now() - 4e8, lastSeenAt: Date.now() - 8.6e7, current: false },
  ];

  let settings: Settings = {
    theme: "system",
    vaultIdleLockSeconds: 300,
    voiceHotkey: "CmdOrCtrl+Shift+Space",
    voiceLanguage: "auto",
    voiceKeepAudio: false,
  };

  let sync: SyncStatus = { state: "disabled", pendingUpdates: 0, lastSyncedAt: null, lastError: null };
  let offline = false;
  let syncTimer: ReturnType<typeof setTimeout> | undefined;

  let models = MODELS.map((model) => ({ ...model }));
  let activeModel: string | null = null;
  let recording = false;
  let transcript = "A synthetic transcript from the preview core.";

  const setSession = (next: SessionStatus) => {
    session = next;
    sync = { ...sync, state: syncStateFor(next.state, sync.pendingUpdates) };
    bus.emit("session:changed", { ...session });
    bus.emit("sync:status", { ...sync });
  };

  function syncStateFor(state: SessionState, pending: number): SyncStatus["state"] {
    if (state !== "signed_in") return "disabled";
    if (offline) return "offline";
    return pending > 0 ? "pending" : "synced";
  }

  const publishSync = (patch: Partial<SyncStatus> = {}) => {
    const pendingUpdates = patch.pendingUpdates ?? sync.pendingUpdates;
    sync = { ...sync, ...patch, state: patch.state ?? syncStateFor(session.state, pendingUpdates), pendingUpdates };
    bus.emit("sync:status", { ...sync });
  };

  const flush = () => {
    clearTimeout(syncTimer);
    syncTimer = undefined;
    if (session.state !== "signed_in" || offline) return;
    publishSync({ pendingUpdates: 0, lastSyncedAt: Date.now(), lastError: null });
  };

  const signedIn = (username: string, serverUrl: string, password: string, label: string) => {
    currentPassword = password;
    devices.unshift({ id: randomId("dev"), deviceLabel: label, createdAt: Date.now(), lastSeenAt: Date.now(), current: true });
    setSession({ state: "signed_in", username, serverUrl });
  };

  const needUser = (username: string) => {
    const user = users.get(username);
    if (!user) throw apiError("unknown_user", "No account with that username on this server.");
    return user;
  };

  const voice = (): VoiceStatus => ({ available: options.voiceAvailable ?? true, recording, activeModel, models: models.map((m) => ({ ...m })) });

  const needModel = (name: string) => {
    const model = models.find((candidate) => candidate.name === name);
    if (!model) throw apiError("unknown_model", "That transcription model does not exist.");
    return model;
  };

  return {
    appVersion: async () => "0.0.0-preview",
    sessionStatus: async () => ({ ...session }),
    async workspaceCreateLocal() {
      if (session.state !== "empty") throw apiError("exists", "A workspace already exists on this device.");
      setSession({ state: "local" });
    },

    async signUp({ serverUrl, setupCode, username, password, deviceLabel }) {
      requireUrl(serverUrl);
      if (setupCode !== MEMORY_SETUP_CODE) throw apiError("invalid_setup_code", "That setup code was not accepted by the server.");
      if (username.trim().length < 3) throw apiError("bad_username", "Use at least 3 characters for the username.");
      if (users.has(username)) throw apiError("username_taken", "That username is already taken.");
      requirePassword(password);
      const recoveryKey = randomRecoveryKey();
      users.set(username, { password, recoveryKey });
      signedIn(username, serverUrl, password, deviceLabel);
      return { recoveryKey };
    },
    async signIn({ serverUrl, username, password, deviceLabel }) {
      requireUrl(serverUrl);
      if (needUser(username).password !== password) throw apiError("wrong_password", "The username or password is not right.");
      signedIn(username, serverUrl, password, deviceLabel);
    },
    async recoverAccount({ serverUrl, username, recoveryKey, newPassword, deviceLabel }) {
      requireUrl(serverUrl);
      const user = needUser(username);
      if (user.recoveryKey !== recoveryKey.trim().toUpperCase()) throw apiError("bad_recovery_key", "That recovery key does not match this account.");
      requirePassword(newPassword);
      user.password = newPassword;
      signedIn(username, serverUrl, newPassword, deviceLabel);
    },
    async signOut() {
      currentPassword = null;
      setSession({ state: "needs_sign_in", username: session.username, serverUrl: session.serverUrl });
    },
    async changePassword({ currentPassword: current, newPassword }) {
      const user = session.username ? users.get(session.username) : undefined;
      if (!user || user.password !== current) throw apiError("wrong_password", "The current password is not right.");
      requirePassword(newPassword);
      user.password = newPassword;
      currentPassword = newPassword;
    },
    sessionsList: async () => devices.map((device) => ({ ...device })),
    async sessionsRevoke({ id }) {
      const at = devices.findIndex((device) => device.id === id);
      if (at < 0) throw apiError("not_found", "That device session no longer exists.");
      if (devices[at]?.current) throw apiError("current_session", "Sign out to end this device's own session.");
      devices.splice(at, 1);
    },

    syncStatus: async () => ({ ...sync }),
    async syncNow() {
      if (session.state !== "signed_in") throw apiError("not_signed_in", "Sync is off for a workspace that lives only on this device.");
      if (offline) {
        publishSync({ state: "offline", lastError: "Cannot reach the server." });
        throw apiError("offline", "Cannot reach the server.");
      }
      flush();
    },
    markDirty() {
      if (session.state !== "signed_in") return;
      publishSync({ pendingUpdates: sync.pendingUpdates + 1 });
      clearTimeout(syncTimer);
      syncTimer = setTimeout(flush, options.syncDelayMs ?? 400);
    },
    accountPassword: () => currentPassword,

    voiceStatus: async () => voice(),
    async voiceDownloadModel({ name }) {
      const model = needModel(name);
      const steps = 4;
      for (let step = 1; step <= steps; step += 1) {
        bus.emit("voice:download-progress", { name, receivedBytes: Math.round((model.byteLength * step) / steps), totalBytes: model.byteLength });
        await new Promise((resolve) => setTimeout(resolve, 0));
      }
      model.installed = true;
      activeModel ??= name;
    },
    async voiceSelectModel({ name }) {
      if (!needModel(name).installed) throw apiError("not_installed", "Download that model before using it.");
      activeModel = name;
    },
    async voiceDeleteModel({ name }) {
      needModel(name).installed = false;
      if (activeModel === name) activeModel = models.find((m) => m.installed)?.name ?? null;
    },
    async voiceStart() {
      if (!activeModel) throw apiError("no_model", "Download a transcription model first.");
      if (recording) throw apiError("busy", "Already recording.");
      recording = true;
    },
    async voiceStop() {
      if (!recording) throw apiError("not_recording", "Nothing is being recorded.");
      recording = false;
      const audio = settings.voiceKeepAudio ? await hooks.recordAudio?.() : undefined;
      return audio ? { text: transcript, audio } : { text: transcript };
    },
    async voiceCancel() {
      recording = false;
    },

    settings: () => settings,
    settingsGet: async () => ({ ...settings }),
    async settingsSet({ settings: patch }) {
      settings = { ...settings, ...patch };
      return { ...settings };
    },

    control: {
      expireSession: () => setSession({ state: "needs_sign_in", username: session.username, serverUrl: session.serverUrl }),
      setOffline(value) {
        offline = value;
        publishSync(value ? { state: "offline" } : {});
        if (!value && sync.pendingUpdates > 0) syncTimer = setTimeout(flush, options.syncDelayMs ?? 400);
      },
      flushSync: flush,
      setNextTranscript: (text) => void (transcript = text),
      pressHotkey: (pressed) => bus.emit("voice:hotkey", { pressed }),
    },
  };
}
