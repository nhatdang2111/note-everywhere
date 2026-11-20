import { matchesQuery } from "../lib/text.ts";
import type {
  CoreApi,
  GeneratePasswordArgs,
  GeneratedPassword,
  VaultCsvFormat,
  VaultItemInput,
  VaultItemSummary,
  VaultSecretField,
  VaultState,
} from "./contract.ts";
import { apiError } from "./errors.ts";
import { MIN_PASSWORD_LENGTH, randomId, randomRecoveryKey, type Bus, type MemoryOptions } from "./memory-bus.ts";

type VaultMethods = Pick<
  CoreApi,
  | "vaultState"
  | "vaultCreate"
  | "vaultUnlock"
  | "vaultRecover"
  | "vaultLock"
  | "vaultChangePassword"
  | "vaultList"
  | "vaultSave"
  | "vaultDelete"
  | "vaultReveal"
  | "vaultCopy"
  | "vaultGeneratePassword"
  | "vaultImportCsv"
>;

export interface VaultControl {
  /** Lock as the core does on idle, sleep or screen lock. */
  autoLock(): void;
  /** What the "clipboard" currently holds, for assertions. */
  clipboard(): string | null;
}

interface StoredItem extends VaultItemSummary {
  password?: string;
  value?: string;
  notes?: string;
}

const WORDS = ["amber", "birch", "cobalt", "delta", "ember", "fjord", "garnet", "harbor", "indigo", "juniper", "kettle", "lantern", "meadow", "nectar", "orchid", "pebble", "quartz", "raven", "saffron", "tundra", "umber", "velvet", "willow", "yarrow", "zephyr"];

function randomInt(max: number): number {
  const buffer = new Uint32Array(1);
  const limit = Math.floor(0x100000000 / max) * max;
  do crypto.getRandomValues(buffer);
  while ((buffer[0] ?? 0) >= limit);
  return (buffer[0] ?? 0) % max;
}

export function generatePassword(args: GeneratePasswordArgs): GeneratedPassword {
  if (args.mode === "passphrase") {
    const count = Math.max(3, Math.min(args.length, 12));
    const words = Array.from({ length: count }, () => WORDS[randomInt(WORDS.length)] ?? "word");
    return { value: words.join("-"), entropyBits: Math.round(count * Math.log2(WORDS.length)) };
  }
  const ambiguous = /[O0Il1]/g;
  const sets = [
    args.lowercase !== false && "abcdefghijklmnopqrstuvwxyz",
    args.uppercase !== false && "ABCDEFGHIJKLMNOPQRSTUVWXYZ",
    args.digits !== false && "0123456789",
    args.symbols && "!@#$%^&*()-_=+[]{};:,.?",
  ]
    .filter((set): set is string => typeof set === "string")
    .map((set) => (args.excludeAmbiguous ? set.replace(ambiguous, "") : set));
  if (sets.length === 0) throw apiError("no_character_sets", "Pick at least one kind of character.");
  const length = Math.max(8, Math.min(args.length, 128));
  const pool = sets.join("");
  const chars = sets.map((set) => set[randomInt(set.length)] ?? "");
  while (chars.length < length) chars.push(pool[randomInt(pool.length)] ?? "");
  for (let i = chars.length - 1; i > 0; i -= 1) {
    const j = randomInt(i + 1);
    [chars[i], chars[j]] = [chars[j] ?? "", chars[i] ?? ""];
  }
  return { value: chars.join(""), entropyBits: Math.round(length * Math.log2(pool.length)) };
}

/** Minimal RFC 4180 reader: quoted fields, doubled quotes, CRLF or LF. */
export function parseCsv(text: string): string[][] {
  const rows: string[][] = [];
  let row: string[] = [];
  let field = "";
  let quoted = false;
  for (let i = 0; i < text.length; i += 1) {
    const ch = text[i];
    if (quoted) {
      if (ch === '"' && text[i + 1] === '"') (field += '"'), (i += 1);
      else if (ch === '"') quoted = false;
      else field += ch;
    } else if (ch === '"') quoted = true;
    else if (ch === ",") (row.push(field), (field = ""));
    else if (ch === "\n" || ch === "\r") {
      if (ch === "\r" && text[i + 1] === "\n") i += 1;
      row.push(field);
      field = "";
      if (row.some((cell) => cell !== "")) rows.push(row);
      row = [];
    } else field += ch;
  }
  row.push(field);
  if (row.some((cell) => cell !== "")) rows.push(row);
  return rows;
}

const CSV_COLUMNS: Record<VaultCsvFormat, { title: string; username: string; password: string; url: string; notes: string }> = {
  generic: { title: "title", username: "username", password: "password", url: "url", notes: "notes" },
  bitwarden: { title: "name", username: "login_username", password: "login_password", url: "login_uri", notes: "notes" },
  one_password: { title: "title", username: "username", password: "password", url: "url", notes: "notes" },
  last_pass: { title: "name", username: "username", password: "password", url: "url", notes: "extra" },
};

const summaryOf = ({ password: _p, value: _v, notes: _n, ...summary }: StoredItem): VaultItemSummary => ({ ...summary });

export function createVault(
  bus: Bus,
  options: MemoryOptions,
  idleSeconds: () => number,
  accountPassword: () => string | null,
): { api: VaultMethods; control: VaultControl } {
  let state: VaultState = "uninitialised";
  let password = "";
  let recoveryKey = "";
  let authedAt = 0;
  let idleTimer: ReturnType<typeof setTimeout> | undefined;
  let clipboard: string | null = null;
  const items = new Map<string, StoredItem>();

  const setState = (next: VaultState) => {
    state = next;
    clearTimeout(idleTimer);
    if (next === "unlocked") {
      authedAt = Date.now();
      touch();
    }
    bus.emit("vault:state", { state });
  };

  function touch() {
    clearTimeout(idleTimer);
    const seconds = idleSeconds();
    if (state === "unlocked" && seconds > 0) idleTimer = setTimeout(() => setState("locked"), seconds * 1000);
  }

  const needUnlocked = () => {
    if (state !== "unlocked") throw apiError("vault_locked", "Unlock the vault first.");
    touch();
  };

  const checkPassword = (candidate: string | undefined) => {
    const grace = options.reauthGraceMs ?? 60_000;
    if (Date.now() - authedAt < grace) return;
    if (candidate === undefined) throw apiError("reauth_required", "Enter your vault password to continue.");
    if (candidate !== password) throw apiError("wrong_password", "That vault password is not right.");
    authedAt = Date.now();
  };

  const needPassword = (value: string) => {
    if (value.length < MIN_PASSWORD_LENGTH) throw apiError("weak_password", `Use at least ${MIN_PASSWORD_LENGTH} characters for the vault password.`);
  };

  const secretOf = (id: string, field: VaultSecretField | "username"): string => {
    const item = items.get(id);
    if (!item) throw apiError("not_found", "That vault item does not exist.");
    return (field === "username" ? item.username : item[field]) ?? "";
  };

  const merge = (existing: StoredItem | undefined, input: VaultItemInput): StoredItem => ({
    id: existing?.id ?? randomId("item"),
    kind: input.kind,
    title: input.title,
    username: input.username ?? existing?.username ?? null,
    urls: input.urls ?? existing?.urls ?? [],
    tags: input.tags ?? existing?.tags ?? [],
    updatedAt: Date.now(),
    // Secrets are kept when the input leaves them out.
    password: input.password ?? existing?.password,
    value: input.value ?? existing?.value,
    notes: input.notes ?? existing?.notes,
  });

  const api: VaultMethods = {
    vaultState: async () => state,
    async vaultCreate({ password: next }) {
      if (state !== "uninitialised") throw apiError("vault_exists", "A vault already exists.");
      needPassword(next);
      password = next;
      recoveryKey = randomRecoveryKey();
      setState("unlocked");
      return { recoveryKey, sameAsAccountPassword: accountPassword() === next };
    },
    async vaultUnlock({ password: attempt }) {
      if (state === "uninitialised") throw apiError("no_vault", "Create the vault first.");
      if (attempt !== password) throw apiError("wrong_password", "That vault password is not right.");
      setState("unlocked");
    },
    async vaultRecover({ recoveryKey: key, newPassword }) {
      if (state === "uninitialised") throw apiError("no_vault", "Create the vault first.");
      if (key.trim().toUpperCase() !== recoveryKey) throw apiError("bad_recovery_key", "That recovery key does not match this vault.");
      needPassword(newPassword);
      password = newPassword;
      setState("unlocked");
    },
    async vaultLock() {
      if (state === "unlocked") setState("locked");
    },
    async vaultChangePassword({ currentPassword, newPassword }) {
      needUnlocked();
      if (currentPassword !== password) throw apiError("wrong_password", "The current vault password is not right.");
      needPassword(newPassword);
      password = newPassword;
    },
    async vaultList({ query }) {
      needUnlocked();
      const text = query?.trim() ?? "";
      return [...items.values()]
        .filter((item) => !text || matchesQuery([item.title, item.username ?? "", ...item.urls, ...item.tags].join(" "), text))
        .sort((a, b) => a.title.localeCompare(b.title))
        .map(summaryOf);
    },
    async vaultSave({ item }) {
      needUnlocked();
      if (!item.title.trim()) throw apiError("title_required", "Give the item a title.");
      const existing = item.id ? items.get(item.id) : undefined;
      if (item.id && !existing) throw apiError("not_found", "That vault item does not exist.");
      const saved = merge(existing, item);
      items.set(saved.id, saved);
      return summaryOf(saved);
    },
    async vaultDelete({ id }) {
      needUnlocked();
      items.delete(id);
    },
    async vaultReveal({ id, field, password: attempt }) {
      needUnlocked();
      checkPassword(attempt);
      return secretOf(id, field);
    },
    async vaultCopy({ id, field, password: attempt }) {
      needUnlocked();
      if (field !== "username") checkPassword(attempt);
      const seconds = options.clipboardSeconds ?? 30;
      const text = secretOf(id, field);
      clipboard = text;
      void navigator.clipboard?.writeText(text).catch(() => undefined);
      setTimeout(() => {
        if (clipboard === text) clipboard = null;
      }, seconds * 1000);
      return { clearAfterSeconds: seconds };
    },
    vaultGeneratePassword: async (args) => generatePassword(args),
    async vaultImportCsv({ format }) {
      needUnlocked();
      const pick = options.pickFiles;
      if (!pick) throw apiError("unsupported", "CSV import needs the native file picker.");
      const [file] = await pick(".csv,text/csv", false);
      if (!file) return null;
      const [header, ...rows] = parseCsv(await file.text());
      const columns = CSV_COLUMNS[format];
      const at = (name: string) => header?.findIndex((cell) => cell.trim().toLowerCase() === name) ?? -1;
      const index = { title: at(columns.title), username: at(columns.username), password: at(columns.password), url: at(columns.url), notes: at(columns.notes) };
      const report = { imported: 0, skipped: [] as { row: number; reason: string }[] };
      rows.forEach((cells, offset) => {
        const cell = (i: number) => (i >= 0 ? (cells[i] ?? "").trim() : "");
        const title = cell(index.title);
        if (!title) return void report.skipped.push({ row: offset + 2, reason: "Missing title" });
        const saved = merge(undefined, {
          kind: "login",
          title,
          username: cell(index.username) || undefined,
          password: cell(index.password) || undefined,
          urls: cell(index.url) ? [cell(index.url)] : [],
          notes: cell(index.notes) || undefined,
        });
        items.set(saved.id, saved);
        report.imported += 1;
      });
      return report;
    },
  };

  return {
    api,
    control: {
      autoLock: () => state === "unlocked" && setState("locked"),
      clipboard: () => clipboard,
    },
  };
}
