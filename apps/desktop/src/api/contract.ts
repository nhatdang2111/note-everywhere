/**
 * The contract between the UI and the Rust core.
 *
 * Every command is a Tauri command with the same name in snake_case
 * (`treeMove` here is `tree_move` there); argument and result shapes are the
 * types below, serialised as camelCase JSON. Binary payloads travel as
 * `Uint8Array`. A failed command rejects with `ApiError`.
 *
 * Document conventions shared by both sides: an inline link to a page is a
 * `link` mark whose `href` is `ne-page:<pageId>`, and an image block's `src`
 * is `attachment:<hash>`.
 *
 * The UI talks to the core only through `CoreApi` and `CoreEvents`, so it can
 * be exercised in tests with an in-memory implementation.
 */

export type NodeId = string;
export type AttachmentHash = string;

export interface ApiError {
  /** Stable machine-readable code, for example `wrong_password`. */
  code: string;
  /** Human-readable English message, safe to show. Never contains secrets. */
  message: string;
}

// ---------------------------------------------------------------- session --

export type SessionState =
  /** No workspace on this device yet. */
  | "empty"
  /** A workspace exists and is open, with no sync account (local only). */
  | "local"
  /** Signed in and syncing. */
  | "signed_in"
  /** The stored session was rejected; the password is needed again. */
  | "needs_sign_in";

export interface SessionStatus {
  state: SessionState;
  username?: string;
  serverUrl?: string;
}

export interface SignUpArgs {
  serverUrl: string;
  setupCode: string;
  username: string;
  password: string;
  deviceLabel: string;
}

export interface SignInArgs {
  serverUrl: string;
  username: string;
  password: string;
  deviceLabel: string;
}

export interface RecoverAccountArgs {
  serverUrl: string;
  username: string;
  recoveryKey: string;
  newPassword: string;
  deviceLabel: string;
}

export interface DeviceSession {
  id: string;
  deviceLabel: string;
  createdAt: number;
  lastSeenAt: number;
  current: boolean;
}

// ------------------------------------------------------------------- tree --

export type NodeKind = "folder" | "page";

export interface TreeNode {
  id: NodeId;
  parentId: NodeId | null;
  kind: NodeKind;
  title: string;
  icon: string | null;
  favorite: boolean;
  createdAt: number;
  updatedAt: number;
}

/** Visible nodes in depth-first order; siblings are in display order. */
export type TreeSnapshot = TreeNode[];

export interface TrashEntry extends TreeNode {
  trashedAt: number;
}

export interface CreateNodeArgs {
  kind: NodeKind;
  parentId: NodeId | null;
  /** Position among visible siblings; omitted means last. */
  index?: number;
  title?: string;
}

export interface MoveNodeArgs {
  id: NodeId;
  parentId: NodeId | null;
  /** Position among the new parent's visible children, counted after the node is detached; omitted means last. */
  index?: number;
}

// ------------------------------------------------------------------- page --

export interface OpenPage {
  /** Full Loro snapshot of the page document, to load into the editor. */
  snapshot: Uint8Array;
}

export interface Backlink {
  pageId: NodeId;
  title: string;
  snippet: string;
}

// ----------------------------------------------------------------- search --

export interface SearchHit {
  pageId: NodeId;
  title: string;
  /** Titles of ancestors, outermost first. */
  breadcrumb: string[];
  snippet: string;
  /** Character ranges inside `snippet` to highlight, as [start, end). */
  highlights: [number, number][];
}

// ------------------------------------------------------------ attachments --

export interface AddedAttachment {
  hash: AttachmentHash;
  mime: string;
  byteLength: number;
  /** URL the webview can load, served by the core. */
  url: string;
}

// ------------------------------------------------------------------- sync --

export type SyncState = "disabled" | "synced" | "pending" | "offline" | "error";

export interface SyncStatus {
  state: SyncState;
  pendingUpdates: number;
  lastSyncedAt: number | null;
  lastError: string | null;
}

// ------------------------------------------------------------------ vault --

export type VaultState = "uninitialised" | "locked" | "unlocked";

export type VaultItemKind = "login" | "secure_note" | "api_key";

/** Never carries a secret value. */
export interface VaultItemSummary {
  id: string;
  kind: VaultItemKind;
  title: string;
  username: string | null;
  urls: string[];
  tags: string[];
  updatedAt: number;
}

export type VaultSecretField = "password" | "value" | "notes";

/** When updating, a secret field that is omitted keeps its stored value. */
export interface VaultItemInput {
  /** Omitted when creating. */
  id?: string;
  kind: VaultItemKind;
  title: string;
  username?: string;
  password?: string;
  value?: string;
  urls?: string[];
  notes?: string;
  tags?: string[];
}

export interface GeneratePasswordArgs {
  mode: "characters" | "passphrase";
  length: number;
  lowercase?: boolean;
  uppercase?: boolean;
  digits?: boolean;
  symbols?: boolean;
  excludeAmbiguous?: boolean;
}

export interface GeneratedPassword {
  value: string;
  entropyBits: number;
}

export type VaultCsvFormat = "generic" | "bitwarden" | "one_password" | "last_pass";

export interface VaultImportReport {
  imported: number;
  skipped: { row: number; reason: string }[];
}

// ------------------------------------------------------------------ voice --

export interface VoiceModel {
  name: string;
  byteLength: number;
  installed: boolean;
}

export interface VoiceStatus {
  /** False when this build has no transcription support. */
  available: boolean;
  recording: boolean;
  activeModel: string | null;
  models: VoiceModel[];
}

export type VoiceLanguage = "auto" | "vi" | "en";

// --------------------------------------------------------------- settings --

export interface Settings {
  theme: "system" | "light" | "dark";
  vaultIdleLockSeconds: number;
  voiceHotkey: string;
  voiceLanguage: VoiceLanguage;
  voiceKeepAudio: boolean;
}

// --------------------------------------------------------------- commands --

export interface CoreApi {
  appVersion(): Promise<string>;

  // Session
  sessionStatus(): Promise<SessionStatus>;
  /** Creates a workspace that lives only on this device. */
  workspaceCreateLocal(): Promise<void>;
  /** Returns the account recovery key, shown to the person once. */
  signUp(args: SignUpArgs): Promise<{ recoveryKey: string }>;
  signIn(args: SignInArgs): Promise<void>;
  recoverAccount(args: RecoverAccountArgs): Promise<void>;
  signOut(): Promise<void>;
  changePassword(args: { currentPassword: string; newPassword: string }): Promise<void>;
  sessionsList(): Promise<DeviceSession[]>;
  sessionsRevoke(args: { id: string }): Promise<void>;

  // Tree
  treeSnapshot(): Promise<TreeSnapshot>;
  treeCreate(args: CreateNodeArgs): Promise<NodeId>;
  treeRename(args: { id: NodeId; title: string }): Promise<void>;
  treeSetIcon(args: { id: NodeId; icon: string | null }): Promise<void>;
  treeSetFavorite(args: { id: NodeId; favorite: boolean }): Promise<void>;
  treeMove(args: MoveNodeArgs): Promise<void>;
  treeTrash(args: { id: NodeId }): Promise<void>;
  treeRestore(args: { id: NodeId }): Promise<void>;
  treeDeletePermanently(args: { id: NodeId }): Promise<void>;
  treeListTrash(): Promise<TrashEntry[]>;
  treeEmptyTrash(): Promise<void>;

  // Pages
  pageOpen(args: { id: NodeId }): Promise<OpenPage>;
  /** A Loro update produced by the editor for this page. */
  pageApplyUpdate(args: { id: NodeId; update: Uint8Array }): Promise<void>;
  pageClose(args: { id: NodeId }): Promise<void>;
  pageBacklinks(args: { id: NodeId }): Promise<Backlink[]>;

  // Search
  searchQuery(args: { text: string; limit?: number }): Promise<SearchHit[]>;

  // Attachments
  attachmentAddBytes(args: { bytes: Uint8Array; mime: string; fileName?: string }): Promise<AddedAttachment>;
  /** Opens the native file picker; resolves to null when cancelled. */
  attachmentPick(args: { imagesOnly: boolean }): Promise<AddedAttachment | null>;
  attachmentUrl(args: { hash: AttachmentHash }): Promise<string>;

  // Import and export; both open a native folder picker and resolve to null when cancelled.
  exportMarkdown(args: { id: NodeId | null }): Promise<{ pagesWritten: number } | null>;
  importMarkdown(args: { parentId: NodeId | null }): Promise<{ pagesCreated: number } | null>;

  // Sync
  syncStatus(): Promise<SyncStatus>;
  syncNow(): Promise<void>;

  // Vault
  vaultState(): Promise<VaultState>;
  /** Returns the vault recovery key, shown to the person once. */
  vaultCreate(args: { password: string }): Promise<{ recoveryKey: string; sameAsAccountPassword: boolean }>;
  vaultUnlock(args: { password: string }): Promise<void>;
  vaultRecover(args: { recoveryKey: string; newPassword: string }): Promise<void>;
  vaultLock(): Promise<void>;
  vaultChangePassword(args: { currentPassword: string; newPassword: string }): Promise<void>;
  vaultList(args: { query?: string }): Promise<VaultItemSummary[]>;
  vaultSave(args: { item: VaultItemInput }): Promise<VaultItemSummary>;
  vaultDelete(args: { id: string }): Promise<void>;
  /** `password` is required when the re-authentication grace period has passed (`reauth_required`). */
  vaultReveal(args: { id: string; field: VaultSecretField; password?: string }): Promise<string>;
  /** The core writes to the clipboard and clears it after `clearAfterSeconds`. */
  vaultCopy(args: { id: string; field: VaultSecretField | "username"; password?: string }): Promise<{ clearAfterSeconds: number }>;
  vaultGeneratePassword(args: GeneratePasswordArgs): Promise<GeneratedPassword>;
  /** Opens the native file picker; resolves to null when cancelled. */
  vaultImportCsv(args: { format: VaultCsvFormat }): Promise<VaultImportReport | null>;

  // Voice
  voiceStatus(): Promise<VoiceStatus>;
  voiceDownloadModel(args: { name: string }): Promise<void>;
  voiceDeleteModel(args: { name: string }): Promise<void>;
  /** Chooses which installed model transcription uses. */
  voiceSelectModel(args: { name: string }): Promise<void>;
  voiceStart(): Promise<void>;
  /** Stops recording and resolves with the transcribed text. */
  voiceStop(): Promise<{ text: string }>;
  voiceCancel(): Promise<void>;

  // Settings
  settingsGet(): Promise<Settings>;
  settingsSet(args: { settings: Partial<Settings> }): Promise<Settings>;
}

// ----------------------------------------------------------------- events --

/** Events the core emits; names are the Tauri event names. */
export interface CoreEventMap {
  /** The tree changed, locally or from sync; re-read `treeSnapshot`. */
  "tree:changed": { nodeIds: NodeId[] };
  /** An update from another device for a page; feed it to that page's editor. */
  "page:remote-update": { pageId: NodeId; update: Uint8Array };
  "sync:status": SyncStatus;
  "session:changed": SessionStatus;
  "vault:state": { state: VaultState };
  "voice:download-progress": { name: string; receivedBytes: number; totalBytes: number };
  /** Hold-to-talk hotkey pressed or released while the app runs. */
  "voice:hotkey": { pressed: boolean };
}

export interface CoreEvents {
  on<K extends keyof CoreEventMap>(event: K, handler: (payload: CoreEventMap[K]) => void): Promise<() => void>;
}
