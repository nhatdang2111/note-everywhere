import type { JSX } from "solid-js";
import type { Settings } from "../../api/contract.ts";
import { LOCALES, locale, setLocale, t, type Locale } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Select } from "../common/form.tsx";
import { VoiceSettings } from "../voice/voice-settings.tsx";
import { AccountSection } from "./account-section.tsx";
import { UpdatesSection } from "./updates-section.tsx";

const IDLE_OPTIONS = [60, 300, 900, 1800, 3600];

/** Theme, language, account, vault lock time, voice, and data. */
export function SettingsPage(): JSX.Element {
  const app = useApp();
  const { settings } = app;
  const idle = () => {
    const seconds = settings.settings().vaultIdleLockSeconds;
    return IDLE_OPTIONS.includes(seconds) ? seconds : IDLE_OPTIONS.reduce((best, option) => (Math.abs(option - seconds) < Math.abs(best - seconds) ? option : best));
  };

  return (
    <section class="page-surface settings" aria-labelledby="settings-title">
      <h1 id="settings-title">{t("settings.title")}</h1>

      <section class="settings-section" aria-labelledby="appearance-title">
        <h2 id="appearance-title">{t("settings.appearance")}</h2>
        <Select<Settings["theme"]>
          label={t("theme.label")}
          value={settings.settings().theme}
          options={[
            { value: "system", label: t("theme.system") },
            { value: "light", label: t("theme.light") },
            { value: "dark", label: t("theme.dark") },
          ]}
          onChange={(theme) => void settings.update({ theme })}
        />
        <Select<Locale> label={t("settings.language")} value={locale()} options={LOCALES.map((l) => ({ value: l.id, label: l.label }))} onChange={setLocale} />
      </section>

      <AccountSection />

      <section class="settings-section" aria-labelledby="vault-settings-title">
        <h2 id="vault-settings-title">{t("settings.vault")}</h2>
        <Select
          label={t("settings.vaultIdle")}
          value={String(idle())}
          hint={t("settings.vaultIdleHint")}
          options={IDLE_OPTIONS.map((seconds) => ({ value: String(seconds), label: t("settings.minutes", { count: seconds / 60 }) }))}
          onChange={(value) => void settings.update({ vaultIdleLockSeconds: Number(value) })}
        />
      </section>

      <VoiceSettings />

      <UpdatesSection />

      <section class="settings-section" aria-labelledby="data-title">
        <h2 id="data-title">{t("settings.data")}</h2>
        <p class="view-intro">{t("settings.dataHint")}</p>
        <div class="header-actions">
          <Button icon="download" onClick={() => void app.importMarkdown(null)}>
            {t("sidebar.importMarkdown")}
          </Button>
          <Button icon="upload" onClick={() => void app.exportMarkdown(null)}>
            {t("sidebar.exportAll")}
          </Button>
        </div>
      </section>
    </section>
  );
}
