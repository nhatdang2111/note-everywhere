import type { JSX } from "solid-js";
import type { SyncState } from "../../api/contract.ts";
import { t, type MessageKey } from "../../i18n/index.ts";
import { formatRelative } from "../../lib/format.ts";
import { useApp } from "../../state/app.tsx";
import { Icon, type IconName } from "../common/icons.tsx";

const VIEW: Record<SyncState, { icon: IconName; label: MessageKey }> = {
  disabled: { icon: "lock", label: "sync.disabled" },
  synced: { icon: "check", label: "sync.synced" },
  pending: { icon: "sync", label: "sync.pending" },
  offline: { icon: "cloud-off", label: "sync.offline" },
  error: { icon: "alert", label: "sync.error" },
};

/** The five sync states as a pill: icon plus text, never colour alone. Clicking syncs now. */
export function SyncIndicator(): JSX.Element {
  const { sync } = useApp();
  const status = () => sync.status();
  const detail = () => {
    const s = status();
    const parts: string[] = [];
    if (s.state === "pending") parts.push(t("sync.pendingCount", { count: s.pendingUpdates }));
    if (s.lastSyncedAt) parts.push(t("sync.lastSynced", { when: formatRelative(s.lastSyncedAt) }));
    if (s.lastError && (s.state === "error" || s.state === "offline")) parts.push(s.lastError);
    return parts.join(" · ");
  };
  return (
    <button
      type="button"
      class="sync-pill"
      data-state={status().state}
      disabled={status().state === "disabled"}
      title={detail() || t(VIEW[status().state].label)}
      aria-label={`${t(VIEW[status().state].label)}${detail() ? `. ${detail()}` : ""}`}
      onClick={() => void sync.syncNow()}
    >
      <Icon name={VIEW[status().state].icon} size={14} />
      <span>{t(VIEW[status().state].label)}</span>
    </button>
  );
}
