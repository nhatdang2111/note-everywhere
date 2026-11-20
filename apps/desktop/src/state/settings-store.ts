import { createEffect, createSignal, onCleanup } from "solid-js";
import type { Core } from "../api/index.ts";
import type { Settings } from "../api/contract.ts";

const THEME_KEY = "ne.theme";

export const DEFAULT_SETTINGS: Settings = {
  theme: "system",
  vaultIdleLockSeconds: 300,
  voiceHotkey: "CmdOrCtrl+Shift+Space",
  voiceLanguage: "auto",
  voiceKeepAudio: false,
};

export function resolveTheme(theme: Settings["theme"]): "light" | "dark" {
  if (theme !== "system") return theme;
  return window.matchMedia("(prefers-color-scheme: dark)").matches ? "dark" : "light";
}

/** Apply the theme remembered from last time before the core answers, to avoid a flash. */
export function applyStoredTheme(): void {
  const stored = localStorage.getItem(THEME_KEY);
  const theme = stored === "light" || stored === "dark" || stored === "system" ? stored : "system";
  document.documentElement.dataset.theme = resolveTheme(theme);
}

export interface SettingsStore {
  settings: () => Settings;
  update(patch: Partial<Settings>): Promise<void>;
  refresh(): Promise<void>;
}

/** Must be created inside a reactive root: it owns the theme effect. */
export function createSettingsStore(core: Core, onError: (error: unknown) => void): SettingsStore {
  const [settings, setSettings] = createSignal<Settings>(DEFAULT_SETTINGS);

  createEffect(() => {
    const theme = settings().theme;
    document.documentElement.dataset.theme = resolveTheme(theme);
    localStorage.setItem(THEME_KEY, theme);
    if (theme !== "system") return;
    const query = window.matchMedia("(prefers-color-scheme: dark)");
    const follow = () => (document.documentElement.dataset.theme = resolveTheme("system"));
    query.addEventListener("change", follow);
    onCleanup(() => query.removeEventListener("change", follow));
  });

  return {
    settings,
    async refresh() {
      try {
        setSettings(await core.api.settingsGet());
      } catch (error) {
        onError(error);
      }
    },
    async update(patch) {
      const previous = settings();
      setSettings({ ...previous, ...patch });
      try {
        setSettings(await core.api.settingsSet({ settings: patch }));
      } catch (error) {
        setSettings(previous);
        onError(error);
      }
    },
  };
}
