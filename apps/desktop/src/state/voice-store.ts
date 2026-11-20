import { createMemo, createSignal } from "solid-js";
import type { Core } from "../api/index.ts";
import type { VoiceStatus } from "../api/contract.ts";
import { insertAttachmentBlock, insertTextAtCursor } from "../editor/index.ts";
import { t } from "../i18n/index.ts";
import type { UiStore } from "./ui-store.ts";

export interface DownloadProgress {
  receivedBytes: number;
  totalBytes: number;
}

export interface VoiceStore {
  status: () => VoiceStatus | null;
  /** False hides the whole feature. */
  available: () => boolean;
  recording: () => boolean;
  /** True from the moment recording stops until the text is in the page. */
  transcribing: () => boolean;
  downloads: () => Record<string, DownloadProgress>;
  refresh(): Promise<void>;
  downloadModel(name: string): Promise<void>;
  deleteModel(name: string): Promise<void>;
  /** Choose which installed model transcribes. */
  selectModel(name: string): Promise<void>;
  /** Begin recording (button pressed or hotkey down). */
  press(): Promise<void>;
  /** Stop and insert the transcript at the cursor (button or hotkey released). */
  release(): Promise<void>;
  cancel(): Promise<void>;
}

interface Callbacks {
  onError(error: unknown): void;
  /** The text could not be placed because no page is open. */
  onOrphanText(text: string): void;
}

export function createVoiceStore(core: Core, ui: UiStore, callbacks: Callbacks): VoiceStore {
  const [status, setStatus] = createSignal<VoiceStatus | null>(null);
  const [recording, setRecording] = createSignal(false);
  const [transcribing, setTranscribing] = createSignal(false);
  const [downloads, setDownloads] = createSignal<Record<string, DownloadProgress>>({});
  const available = createMemo(() => status()?.available === true);

  let starting: Promise<void> | null = null;
  let releaseRequested = false;

  async function refresh(): Promise<void> {
    try {
      const next = await core.api.voiceStatus();
      setStatus(next);
      setRecording(next.recording);
    } catch (error) {
      callbacks.onError(error);
    }
  }

  void core.events.on("voice:download-progress", ({ name, receivedBytes, totalBytes }) =>
    setDownloads((current) => ({ ...current, [name]: { receivedBytes, totalBytes } })),
  );

  async function stopAndInsert(): Promise<void> {
    setTranscribing(true);
    try {
      const { text, audio } = await core.api.voiceStop();
      setRecording(false);
      const trimmed = text.trim();
      const editor = ui.activeEditor();
      if (!trimmed && !audio) return;
      if (!editor) {
        if (trimmed) callbacks.onOrphanText(trimmed);
        return;
      }
      if (trimmed) insertTextAtCursor(editor.view, trimmed);
      // The recording goes in after the text, as a link to the stored file.
      if (audio) insertAttachmentBlock(editor.view, audio.hash, t("voice.recordingLabel"));
      editor.view.focus();
    } catch (error) {
      setRecording(false);
      callbacks.onError(error);
    } finally {
      setTranscribing(false);
    }
  }

  return {
    status,
    available,
    recording,
    transcribing,
    downloads,
    refresh,
    async downloadModel(name) {
      setDownloads((current) => ({ ...current, [name]: { receivedBytes: 0, totalBytes: 0 } }));
      try {
        await core.api.voiceDownloadModel({ name });
      } catch (error) {
        callbacks.onError(error);
      } finally {
        setDownloads(({ [name]: _done, ...rest }) => rest);
        await refresh();
      }
    },
    async deleteModel(name) {
      try {
        await core.api.voiceDeleteModel({ name });
      } catch (error) {
        callbacks.onError(error);
      }
      await refresh();
    },
    async selectModel(name) {
      try {
        await core.api.voiceSelectModel({ name });
      } catch (error) {
        callbacks.onError(error);
      }
      await refresh();
    },
    async press() {
      if (!available() || recording() || starting || transcribing()) return;
      releaseRequested = false;
      starting = core.api
        .voiceStart()
        .then(() => void setRecording(true))
        .catch((error: unknown) => callbacks.onError(error))
        .finally(() => {
          starting = null;
        });
      await starting;
      // Released before the recorder came up: stop right away.
      if (releaseRequested && recording()) await stopAndInsert();
    },
    async release() {
      releaseRequested = true;
      if (starting) return;
      if (recording()) await stopAndInsert();
    },
    async cancel() {
      releaseRequested = false;
      if (!recording()) return;
      try {
        await core.api.voiceCancel();
      } catch (error) {
        callbacks.onError(error);
      }
      setRecording(false);
    },
  };
}
