import { For, type JSX } from "solid-js";
import type { Settings } from "../../api/contract.ts";
import { t, type MessageKey } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Icon, type IconName } from "../common/icons.tsx";

const OPTIONS: { value: Settings["theme"]; icon: IconName; label: MessageKey }[] = [
  { value: "light", icon: "sun", label: "theme.light" },
  { value: "system", icon: "monitor", label: "theme.system" },
  { value: "dark", icon: "moon", label: "theme.dark" },
];

/** Three-way theme switch (radio group so arrow keys work). */
export function ThemeSwitch(): JSX.Element {
  const { settings } = useApp();
  return (
    <div class="theme-switch" role="radiogroup" aria-label={t("theme.label")}>
      <For each={OPTIONS}>
        {(option) => (
          <button
            type="button"
            role="radio"
            aria-checked={settings.settings().theme === option.value}
            aria-label={t(option.label)}
            title={t(option.label)}
            class="theme-option"
            onClick={() => void settings.update({ theme: option.value })}
            onKeyDown={(event) => {
              if (event.key !== "ArrowRight" && event.key !== "ArrowLeft") return;
              event.preventDefault();
              const at = OPTIONS.findIndex((o) => o.value === option.value);
              const next = OPTIONS[(at + (event.key === "ArrowRight" ? 1 : OPTIONS.length - 1)) % OPTIONS.length];
              if (next) void settings.update({ theme: next.value });
            }}
          >
            <Icon name={option.icon} size={14} />
          </button>
        )}
      </For>
    </div>
  );
}
