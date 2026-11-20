import { Show, type JSX } from "solid-js";
import { t } from "../../i18n/index.ts";
import { useApp } from "../../state/app.tsx";
import { Button } from "../common/button.tsx";
import { Icon } from "../common/icons.tsx";

/**
 * Hold-to-talk button: recording runs while the pointer or Space/Enter is held.
 * Hidden entirely when this build has no transcription.
 */
export function TalkButton(): JSX.Element {
  const { voice } = useApp();
  return (
    <Show when={voice.available()}>
      <button
        type="button"
        class="talk-button"
        aria-pressed={voice.recording()}
        aria-label={t("voice.holdToTalk")}
        title={t("voice.holdToTalk")}
        disabled={voice.transcribing()}
        onPointerDown={(event) => {
          event.currentTarget.setPointerCapture?.(event.pointerId);
          void voice.press();
        }}
        onPointerUp={() => void voice.release()}
        onPointerCancel={() => void voice.cancel()}
        onKeyDown={(event) => {
          if ((event.key === " " || event.key === "Enter") && !event.repeat) {
            event.preventDefault();
            void voice.press();
          }
        }}
        onKeyUp={(event) => {
          if (event.key === " " || event.key === "Enter") {
            event.preventDefault();
            void voice.release();
          }
        }}
      >
        <Icon name="mic" size={14} />
        <span>{t("voice.talk")}</span>
      </button>
    </Show>
  );
}

/** Shows that the microphone is live (or that text is being produced), with a way to cancel. */
export function RecordingIndicator(): JSX.Element {
  const { voice } = useApp();
  return (
    <Show when={voice.available() && (voice.recording() || voice.transcribing())}>
      <div class="recording-pill" role="status" data-phase={voice.recording() ? "recording" : "transcribing"}>
        <span class="recording-dot" aria-hidden="true" />
        <span>{voice.recording() ? t("voice.recording") : t("voice.transcribing")}</span>
        <Show when={voice.recording()}>
          <Button variant="ghost" onClick={() => void voice.cancel()}>
            {t("common.cancel")}
          </Button>
        </Show>
      </div>
    </Show>
  );
}
