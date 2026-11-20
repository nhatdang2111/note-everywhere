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

/** A database holds rows (pages) that `treeSnapshot` does not list; see the databases section. */
export type NodeKind = "folder" | "page" | "database";

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

// -------------------------------------------------------------- databases --
//
// A database is a node of the tree with `kind: "database"`. Its rows are page
// nodes that are children of it: they open with `pageOpen` like any page and are
// found by `searchQuery`. Rows are NOT part of `treeSnapshot` (that would be
// thousands of nodes); read them with `dbQuery`. The schema, the cell values
// and the views sit in the database's own document, so `dbQuery` never opens a
// row page. Moving a page under a database with `treeMove` makes it a row with
// no values; only pages can be children of a database.
//
// Concurrency (what the person can see after two devices edit offline):
//   - two edits to different cells (even of one row) both stay;
//   - two edits to the same cell leave one value, the same on both devices;
//   - a property type change against a cell edit: the property's type wins, the
//     cell is read through it (see the conversion table below), nothing breaks;
//   - a property or option deleted while another device writes to it: deleted;
//   - moving the same card to two places: one place wins on both devices.
//
// Property type changes convert on read and never rewrite cells, so changing
// back restores the original values. text->select/multi-select first creates
// the options its values name. Conversions (blank means the cell reads empty):
//   text:         number<-parsed if numeric; select<-option of that name;
//                 multi_select<-options named in the comma list; date<-parsed
//                 if ISO; checkbox<-true for true/yes/1/x; url<-same
//   number:       text<-decimal; checkbox<-nonzero
//   select:       text/url<-option name; multi_select<-that option
//   multi_select: text/url<-names joined by ", "; select<-first option
//   date:         text<-start (and " -> end")
//   checkbox:     text<-"true"/"false"; number<-1/0
//   url:          like text, plus url->text
//   relation:     only relation
// Options are matched by name ignoring case and accents.

export type PropertyType =
  | "text"
  | "number"
  | "select"
  | "multi_select"
  | "date"
  | "checkbox"
  | "url"
  | "relation";

export type OptionColor = "gray" | "brown" | "orange" | "yellow" | "green" | "blue" | "purple" | "pink" | "red";

export interface SelectOption {
  /** Stable id; cell values refer to it, so renaming keeps values. */
  id: string;
  name: string;
  color: OptionColor;
}

/**
 * A two-way relation is two properties. The forward one stores the links and
 * names its partner in `reverseProperty`; the partner (in the other database)
 * has `backlinkOf` and stores nothing: its values are computed, and writing to
 * it edits the forward cells of the other database.
 */
export interface RelationConfig {
  /** The database whose rows the links point at (for a backlink, the database that holds the forward property). */
  database: NodeId;
  reverseProperty?: string;
  backlinkOf?: string;
}

export interface DbProperty {
  id: string;
  name: string;
  type: PropertyType;
  /** In display order; kept while the property has another type. */
  options: SelectOption[];
  relation?: RelationConfig;
}

/** `YYYY-MM-DD` or `YYYY-MM-DDTHH:MM` wall-clock text, no time zone. */
export interface DateValue {
  start: string;
  end?: string;
}

/** The id `"title"` names the title property wherever a property id is expected; it is the title of the row's page. */
export type TitlePropertyId = "title";

export type CellValue =
  | { type: "text"; value: string }
  | { type: "number"; value: number }
  | { type: "select"; value: string }
  | { type: "multi_select"; value: string[] }
  | { type: "date"; value: DateValue }
  | { type: "checkbox"; value: boolean }
  | { type: "url"; value: string }
  | { type: "relation"; value: NodeId[] };

export type ViewKind = "table" | "board" | "list" | "calendar";

export type FilterLogic = "and" | "or";

/**
 * Which operators suit which property type:
 *   title, text, url: contains, not_contains, is, is_not, starts_with, ends_with, is_empty, is_not_empty
 *     (text matching ignores case and accents)
 *   number: is, is_not, gt, gte, lt, lte, is_empty, is_not_empty
 *   select: is, is_not (an option id), is_empty, is_not_empty
 *   multi_select: contains, not_contains (an option id), is_empty, is_not_empty
 *   date: is, before, after, on_or_before, on_or_after (a YYYY-MM-DD day), is_empty, is_not_empty
 *   checkbox: is_checked, is_unchecked
 *   relation: contains, not_contains (a row id), is_empty, is_not_empty
 */
export type FilterOperator =
  | "contains"
  | "not_contains"
  | "is"
  | "is_not"
  | "starts_with"
  | "ends_with"
  | "gt"
  | "gte"
  | "lt"
  | "lte"
  | "before"
  | "after"
  | "on_or_before"
  | "on_or_after"
  | "is_empty"
  | "is_not_empty"
  | "is_checked"
  | "is_unchecked";

/** Groups nest at most three deep. An empty group matches every row. */
export type FilterNode =
  | { type: "group"; logic: FilterLogic; children: FilterNode[] }
  | { type: "condition"; property: string; operator: FilterOperator; value?: string | number | boolean };

/** Text sorts by the Vietnamese alphabet (a, ă, â, b, c, d, đ, e, ê ...) then tone marks; select by option order; rows with no value come last. Ties keep the manual order of the rows. */
export interface SortKey {
  property: string;
  descending: boolean;
}

export interface GroupConfig {
  /** A select, multi_select or checkbox property. */
  property: string;
  hideEmpty: boolean;
}

export interface ColumnConfig {
  property: string;
  visible: boolean;
  width?: number;
}

/**
 * A saved view. `columns` lists columns in display order; properties it does
 * not list are shown after them in schema order, so a new property appears in
 * every view.
 */
export interface ViewDef {
  id: string;
  name: string;
  kind: ViewKind;
  filter: FilterNode | null;
  sorts: SortKey[];
  group: GroupConfig | null;
  columns: ColumnConfig[];
  /** The date property a calendar places rows by. */
  dateProperty: string | null;
}

export interface DatabaseSchema {
  properties: DbProperty[];
  views: ViewDef[];
}

export interface DateRange {
  from: string;
  to: string;
}

/** Everything is optional; `limit` defaults to 100 and is at most 1000 (per group when grouped). */
export interface QuerySpec {
  filter?: FilterNode | null;
  sorts?: SortKey[];
  /** When set the rows come back in `groups` (board columns) instead of `rows`. */
  group?: GroupConfig | null;
  /** With `dateRange`: keep rows whose date (or date range) touches these days. */
  dateProperty?: string | null;
  dateRange?: DateRange | null;
  offset?: number;
  limit?: number;
}

export interface QueryRow {
  id: NodeId;
  title: string;
  icon: string | null;
  /** Effective values by property id (empty cells absent). A backlink property lists the rows that link here. */
  values: Record<string, CellValue>;
}

export interface QueryBucket {
  /** Option id, "true"/"false" for a checkbox, null for rows with no value. */
  key: string | null;
  count: number;
  rows: QueryRow[];
}

export interface QueryResult {
  total: number;
  rows: QueryRow[];
  groups: QueryBucket[];
  /** Titles of rows that relation values point at; trashed or missing rows are absent (and removed from the values). */
  titles: Record<NodeId, string>;
}

/**
 * The query to run: `spec` if given, else the settings of the view `viewId`
 * (conditions on properties that no longer exist are ignored). `offset` and
 * `limit` override whichever applies. A `spec` is validated strictly (unknown
 * property or an operator that does not suit the type is `invalid_input`).
 */
export interface DbQueryArgs {
  id: NodeId;
  viewId?: string;
  spec?: QuerySpec;
  offset?: number;
  limit?: number;
}

export interface DbPropertyAddArgs {
  id: NodeId;
  name: string;
  type: PropertyType;
}

export interface DbPropertyChangeTypeArgs {
  id: NodeId;
  propertyId: string;
  type: PropertyType;
  /** Required when `type` is "relation". */
  relationDatabaseId?: NodeId;
}

export interface DbRowCreateArgs {
  id: NodeId;
  title?: string;
  /** Position among the rows; omitted means last. */
  index?: number;
  /** Default values by property id. */
  values?: Record<string, CellValue>;
}

// -------------------------------------------------------------- templates --

export interface TemplateSummary {
  id: string;
  name: string;
  kind: "page" | "row";
  /** Set for a row template. */
  database?: NodeId;
  /** Default values of a row template, by property id. */
  defaults: Record<string, CellValue>;
  createdAt: number;
}

// ---------------------------------------------------------------- history --

export interface PageVersion {
  /** Opaque; pass to `pageVersionPreview` and `pageVersionRestore`. */
  id: string;
  /** The author's replica id (the editor uses a new one each time a page is opened), not a person. */
  author: string;
  /** ms since the Unix epoch; null when the change carries no time. */
  startedAt: number | null;
  endedAt: number | null;
  changes: number;
  operations: number;
}

/**
 * Versions group consecutive changes by one author with gaps under five
 * minutes. History reaches back to the creation of the page: nothing trims
 * it. `trimmed` and `previewable` are false/true unless a trimmed copy of a
 * page arrives (not produced by this app), in which case versions cannot be
 * previewed or restored. Editors must call `setRecordTimestamp(true)` and
 * `setChangeMergeInterval(30)` on their Loro documents, or their changes carry
 * no time.
 */
export interface PageHistory {
  /** Newest first. */
  versions: PageVersion[];
  trimmed: boolean;
  previewable: boolean;
  oldestAt: number | null;
}

export interface PreviewMarks {
  bold: boolean;
  italic: boolean;
  code: boolean;
  strike: boolean;
  link: string | null;
}

export type PreviewInline = { type: "text"; text: string; marks: PreviewMarks } | { type: "break" };

export type PreviewBlockKind =
  | { type: "paragraph" }
  | { type: "heading"; level: number }
  | { type: "bullet_list" }
  | { type: "ordered_list" }
  | { type: "list_item" }
  | { type: "todo_list" }
  | { type: "todo_item"; checked: boolean }
  | { type: "toggle"; open: boolean }
  | { type: "quote" }
  | { type: "callout"; icon: string }
  | { type: "divider" }
  | { type: "code_block"; language: string }
  | { type: "image"; src: string; alt: string }
  | { type: "child_page"; pageId: string; title: string }
  | { type: "page_link"; pageId: string; title: string }
  | { type: "synced_block"; syncId: string }
  | { type: "unknown"; nodeName: string };

/** A read-only snapshot of a page at a version, as a block tree. */
export interface PreviewBlock {
  kind: PreviewBlockKind;
  /** Inline content of a text block. */
  inline: PreviewInline[];
  children: PreviewBlock[];
}

// ----------------------------------------------------------------- import --

export interface StructuredImportReport {
  folders: number;
  pages: number;
  databases: number;
  rows: number;
  attachments: number;
  linksRewritten: number;
  /** Links to exported pages that point at nothing in the export. */
  linksUnresolved: number;
  imagesMissing: number;
  /** Everything passed over, with the reason: a file too large, a CSV that does not parse, a duplicate. */
  skipped: string[];
}

// ----------------------------------------------------------- synced blocks --
//
// The content of a synced block is its own document, laid out like a page; a
// page holds a placeholder block `synced_block` with a `syncId`. See
// docs/decisions/0008-synced-blocks.md for what the editor has to do.
// Commands that change a page from the core (`syncedCreate`, `syncedInsert`,
// `syncedUnsync`, `pageVersionRestore`) deliver the change to an open editor as
// a `page:remote-update` event for that page.

export interface SyncedOccurrence {
  pageId: NodeId;
  title: string;
  /** The page is in the trash (it still counts as an occurrence). */
  trashed: boolean;
}

export interface SyncedBlockInfo {
  syncId: string;
  occurrences: number;
  /** ms since which no page shows it; such a block is deleted after 30 days. */
  orphanedAt: number | null;
}

// -------------------------------------------------------------- reminders --

export interface Reminder {
  id: string;
  pageId: NodeId;
  /** The date property of the row it follows, if any. */
  propertyId?: string;
  /** ms; the moment it fires (for a property reminder, the moment when it was made). */
  at: number;
  utcOffsetMinutes: number;
  note: string;
  done: boolean;
  snoozedUntil?: number;
  createdAt: number;
}

export interface ReminderEntry {
  reminder: Reminder;
  /** When it is due now: follows the date property, honours a snooze. */
  dueAt: number;
  pageTitle: string;
  /** False when the page is gone or in the trash; such a reminder never fires. */
  pageVisible: boolean;
}

export interface ReminderAddArgs {
  pageId: NodeId;
  /** A date property of the row `pageId`; then `at` may be left out and is read from the cell. A date without a time is 09:00. */
  propertyId?: string;
  /** ms since the Unix epoch. */
  at?: number;
  /** The person's UTC offset, `-new Date().getTimezoneOffset()`. */
  utcOffsetMinutes: number;
  note?: string;
}

// ---------------------------------------------------------------- updates --
//
// The app can update itself from the newest GitHub release of this project.
// Nothing is ever installed without the person asking: `updateCheck` only
// learns that a version exists, `updateDownload` fetches it and checks its
// signature against the public key built into the app, and `updateInstall`
// applies it and restarts. The core also checks once on launch, at most once a
// day, and publishes the outcome as `update:status`; it never downloads by
// itself. A build without an updater key (a local build, a fork) reports
// `unavailable` and never contacts the network.

export type UpdateState =
  /** This build cannot update itself; `message` says why. */
  | "unavailable"
  | "idle"
  | "checking"
  | "up_to_date"
  /** A newer `version` exists. */
  | "available"
  | "downloading"
  /** Downloaded and verified; `updateInstall` applies it. */
  | "ready"
  /** The last check failed; `message` says why. */
  | "error";

export interface UpdateStatus {
  state: UpdateState;
  /** The version of the running app (the same as `appVersion`). */
  currentVersion: string;
  /** The announced version, present while `available`, `downloading` or `ready`. */
  version?: string;
  /** Release notes of the announced version, when it has any. */
  notes?: string;
  /** ms since the Unix epoch of the last completed check, null if none yet. */
  lastCheckedAt: number | null;
  /**
   * Why updates are unavailable, why the last check failed, or why the last
   * download failed (then `state` is back to `available`, so it can be retried).
   */
  message: string | null;
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
  /** Stops recording and resolves with the transcribed text, plus the recording when `voiceKeepAudio` is on. */
  voiceStop(): Promise<{ text: string; audio?: AddedAttachment }>;
  voiceCancel(): Promise<void>;

  // Settings
  settingsGet(): Promise<Settings>;
  settingsSet(args: { settings: Partial<Settings> }): Promise<Settings>;

  // Databases. Rows are pages under the database node; see the databases section.
  dbCreate(args: { parentId: NodeId | null; index?: number; title?: string }): Promise<NodeId>;
  dbSchema(args: { id: NodeId }): Promise<DatabaseSchema>;
  dbQuery(args: DbQueryArgs): Promise<QueryResult>;
  dbPropertyAdd(args: DbPropertyAddArgs): Promise<string>;
  dbPropertyRename(args: { id: NodeId; propertyId: string; name: string }): Promise<void>;
  dbPropertyDelete(args: { id: NodeId; propertyId: string }): Promise<void>;
  dbPropertyMove(args: { id: NodeId; propertyId: string; index: number }): Promise<void>;
  dbPropertyChangeType(args: DbPropertyChangeTypeArgs): Promise<void>;
  /** Adds a relation property to `id` that links to rows of `targetId` (possibly `id`); with `reverseName` it is two-way. Returns the new property's id. */
  dbRelationAdd(args: { id: NodeId; name: string; targetId: NodeId; reverseName?: string }): Promise<string>;
  dbOptionAdd(args: { id: NodeId; propertyId: string; name: string; color?: OptionColor }): Promise<string>;
  dbOptionRename(args: { id: NodeId; propertyId: string; optionId: string; name: string }): Promise<void>;
  dbOptionColor(args: { id: NodeId; propertyId: string; optionId: string; color: OptionColor }): Promise<void>;
  /** Cells that held the option read without it from now on. */
  dbOptionDelete(args: { id: NodeId; propertyId: string; optionId: string }): Promise<void>;
  dbOptionMove(args: { id: NodeId; propertyId: string; optionId: string; index: number }): Promise<void>;
  dbRowCreate(args: DbRowCreateArgs): Promise<NodeId>;
  /** `value: null` clears the cell. `propertyId: "title"` renames the row's page. */
  dbCellSet(args: { id: NodeId; rowId: NodeId; propertyId: string; value: CellValue | null }): Promise<void>;
  /** Sets the grouping property of the card to `group` (option id, "true"/"false" for a checkbox, null for none) and puts it before the row `beforeId`, or last. */
  dbCardMove(args: { id: NodeId; rowId: NodeId; propertyId: string; group: string | null; beforeId?: NodeId | null }): Promise<void>;
  dbViewAdd(args: { id: NodeId; name: string; kind: ViewKind }): Promise<string>;
  /** Saves every field of the view; only fields that differ are written, so edits of different fields on two devices both stay. */
  dbViewSave(args: { id: NodeId; view: ViewDef }): Promise<void>;
  dbViewDelete(args: { id: NodeId; viewId: string }): Promise<void>;
  dbViewMove(args: { id: NodeId; viewId: string; index: number }): Promise<void>;

  // Templates. Without `databaseId`, page templates; with it, that database's row templates.
  templateList(args: { databaseId?: NodeId }): Promise<TemplateSummary[]>;
  templateCreateFromPage(args: { pageId: NodeId; name: string }): Promise<string>;
  /** With `fromRowId` the template takes that row's content and values (except relations). */
  templateCreateForDatabase(args: { databaseId: NodeId; name: string; fromRowId?: NodeId }): Promise<string>;
  templateRename(args: { id: string; name: string }): Promise<void>;
  templateSetDefaults(args: { id: string; defaults: Record<string, CellValue> }): Promise<void>;
  templateDelete(args: { id: string }): Promise<void>;
  templateInstantiatePage(args: { id: string; parentId: NodeId | null; index?: number; title?: string }): Promise<NodeId>;
  templateInstantiateRow(args: { id: string; title?: string }): Promise<NodeId>;

  // Page history
  pageHistory(args: { id: NodeId }): Promise<PageHistory>;
  pageVersionPreview(args: { id: NodeId; versionId: string }): Promise<PreviewBlock[]>;
  /** A new forward change that sync delivers to every device; an open editor receives it as `page:remote-update`. */
  pageVersionRestore(args: { id: NodeId; versionId: string }): Promise<void>;

  // Structured import of another app's Markdown + CSV export; opens a native folder picker and resolves to null when cancelled.
  importStructured(args: { parentId: NodeId | null }): Promise<StructuredImportReport | null>;

  // Synced blocks
  /** Turns the top-level blocks `start..end` of the page into a synced block. */
  syncedCreate(args: { pageId: NodeId; start: number; end: number }): Promise<{ syncId: string }>;
  syncedInsert(args: { pageId: NodeId; parentPath: number[]; index: number; syncId: string }): Promise<void>;
  /** Full snapshot of the shared document; marks it open so its remote updates arrive as `page:remote-update` with `pageId: syncId`. */
  syncedOpen(args: { syncId: string }): Promise<OpenPage>;
  syncedApplyUpdate(args: { syncId: string; update: Uint8Array }): Promise<void>;
  syncedClose(args: { syncId: string }): Promise<void>;
  /** Replaces the placeholder at `path` with a copy of the content. */
  syncedUnsync(args: { pageId: NodeId; path: number[] }): Promise<void>;
  syncedOccurrences(args: { syncId: string }): Promise<SyncedOccurrence[]>;
  syncedList(): Promise<SyncedBlockInfo[]>;

  // Reminders
  reminderAdd(args: ReminderAddArgs): Promise<Reminder>;
  reminderList(args: { includeDone?: boolean }): Promise<ReminderEntry[]>;
  reminderSetDone(args: { id: string; done: boolean }): Promise<void>;
  /** Snoozes until `until` (ms since the Unix epoch). */
  reminderSnooze(args: { id: string; until: number }): Promise<void>;
  reminderReschedule(args: { id: string; at: number }): Promise<void>;
  reminderSetNote(args: { id: string; note: string }): Promise<void>;
  reminderDelete(args: { id: string }): Promise<void>;

  // Updates. The running version is `appVersion()` and `UpdateStatus.currentVersion`.
  updateStatus(): Promise<UpdateStatus>;
  /** Asks the update server now. A failure comes back as `state: "error"`, not as a rejection. Does nothing while `unavailable` or `ready`; rejects with `update_busy` while a check or download runs. */
  updateCheck(): Promise<UpdateStatus>;
  /** Downloads and verifies the announced version, with `update:progress` events. Rejects with `no_update` unless `available`. A failure comes back as `state: "available"` with `message` set. */
  updateDownload(): Promise<UpdateStatus>;
  /** Installs the downloaded version and restarts the app; resolves only if the restart did not happen. Rejects with `update_not_ready` unless `ready`, and with `update_failed` if the install failed (the state returns to `available`). */
  updateInstall(): Promise<void>;
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
  /**
   * A database changed because of an edit made here (schema, a cell, a row);
   * re-run `dbSchema` and `dbQuery`. Edits that arrive from other devices come
   * as `tree:changed`, whose `nodeIds` then include the database and the rows
   * whose cells changed.
   */
  "database:changed": { databaseId: NodeId; rowIds: NodeId[] };
  /** A reminder was added, changed or deleted here or on another device (noticed within about 30 seconds); re-run `reminderList`. */
  "reminders:changed": Record<string, never>;
  /**
   * Reminders that are due now. Each fires once per device and never for a
   * moment older than ten minutes when the device looked. The core shows the
   * native notification itself when the system allows it; `nativeShown` is true
   * when it did for every reminder in the event, so the UI must not show a
   * second one. When it is false (permission denied or not given), show your own.
   */
  "reminder:due": { reminders: ReminderEntry[]; nativeShown: boolean };
  /** The update state changed (a check started or ended, a download started or ended). */
  "update:status": UpdateStatus;
  /** Progress of `updateDownload`; `totalBytes` is null when the server did not say. */
  "update:progress": { receivedBytes: number; totalBytes: number | null };
}

export interface CoreEvents {
  on<K extends keyof CoreEventMap>(event: K, handler: (payload: CoreEventMap[K]) => void): Promise<() => void>;
}
