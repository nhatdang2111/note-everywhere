import { Index, Show, createSignal, onCleanup, type JSX } from "solid-js";
import type { VoiceLanguage } from "../../api/contract.ts";
import { t } from "../../i18n/index.ts";
import { isMac } from "../../lib/device.ts";
import { formatBytes } from "../../lib/format.ts";
import { acceleratorFromEvent, describeAccelerator } from "../../lib/hotkey.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Checkbox, Select } from "../common/form.tsx";

function HotkeyField(): JSX.Element {
  const { settings } = useApp();
  const [capturing, setCapturing] = createSignal(false);

  const onKeyDown = (event: KeyboardEvent) => {
    if (!capturing()) return;
    event.preventDefault();
    event.stopPropagation();
    if (event.key === "Escape") return void setCapturing(false);
    const accelerator = acceleratorFromEvent(event, isMac());
    if (!accelerator) return;
    setCapturing(false);
    void settings.update({ voiceHotkey: accelerator });
  };

  document.addEventListener("keydown", onKeyDown, true);
  onCleanup(() => document.removeEventListener("keydown", onKeyDown, true));

  return (
    <div class="field">
      <span class="field-label" id="hotkey-label">
        {t("voice.hotkey")}
      </span>
      <Button
        aria-labelledby="hotkey-label hotkey-value"
        onClick={() => setCapturing(!capturing())}
        onBlur={() => setCapturing(false)}
      >
        <span id="hotkey-value">{capturing() ? t("voice.pressKeys") : describeAccelerator(settings.settings().voiceHotkey, isMac())}</span>
      </Button>
      <p class="field-hint">{t("voice.hotkeyHint")}</p>
    </div>
  );
}

/** Model download with progress, language, hotkey and audio options. Hidden when the build has no transcription. */
export function VoiceSettings(): JSX.Element {
  const { voice, settings } = useApp();
  const status = () => voice.status();
  return (
    <Show when={voice.available()}>
      <section class="settings-section" aria-labelledby="voice-settings-title">
        <h2 id="voice-settings-title">{t("voice.settingsTitle")}</h2>
        <p class="view-intro">{t("voice.settingsIntro")}</p>

        <ul class="model-list" aria-label={t("voice.models")}>
          <Index each={status()?.models ?? []}>
            {(model) => {
              const progress = () => voice.downloads()[model().name];
              return (
                <li class="model-row">
                  <span class="model-name">{model().name}</span>
                  <span class="muted">{formatBytes(model().byteLength)}</span>
                  <Show
                    when={!progress()}
                    fallback={
                      <progress
                        value={progress()?.receivedBytes ?? 0}
                        max={progress()?.totalBytes || model().byteLength}
                        aria-label={t("voice.downloading", { name: model().name })}
                      />
                    }
                  >
                    <Show
                      when={model().installed}
                      fallback={
                        <Button icon="download" onClick={() => void voice.downloadModel(model().name)}>
                          {t("voice.download")}
                        </Button>
                      }
                    >
                      <span class="badge" data-active={status()?.activeModel === model().name ? "true" : undefined}>
                        {status()?.activeModel === model().name ? t("voice.active") : t("voice.installed")}
                      </span>
                      <Show when={status()?.activeModel !== model().name}>
                        <Button onClick={() => void voice.selectModel(model().name)}>{t("voice.use")}</Button>
                      </Show>
                      <Button variant="danger" onClick={() => void voice.deleteModel(model().name)}>
                        {t("common.delete")}
                      </Button>
                    </Show>
                  </Show>
                </li>
              );
            }}
          </Index>
        </ul>

        <Select<VoiceLanguage>
          label={t("voice.language")}
          value={settings.settings().voiceLanguage}
          options={[
            { value: "auto", label: t("voice.langAuto") },
            { value: "vi", label: "Tiếng Việt" },
            { value: "en", label: "English" },
          ]}
          onChange={(voiceLanguage) => void settings.update({ voiceLanguage })}
        />
        <HotkeyField />
        <Checkbox label={t("voice.keepAudio")} checked={settings.settings().voiceKeepAudio} onChange={(voiceKeepAudio) => void settings.update({ voiceKeepAudio })} />
      </section>
    </Show>
  );
}
