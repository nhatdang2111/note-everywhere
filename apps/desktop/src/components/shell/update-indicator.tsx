import { Show, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Icon } from "../common/icons.tsx";

/** A quiet pill in the top bar while an update waits; it opens Settings and never acts by itself. */
export function UpdateIndicator(): JSX.Element {
  const app = useApp();
  const waiting = () => ["available", "downloading", "ready"].includes(app.updates.status()?.state ?? "");
  return (
    <Show when={waiting()}>
      <button type="button" class="update-pill" onClick={() => app.openTarget({ kind: "settings" })}>
        <Icon name="download" size={14} />
        <span>{t(app.updates.status()?.state === "ready" ? "updates.pillReady" : "updates.pill")}</span>
      </button>
    </Show>
  );
}
