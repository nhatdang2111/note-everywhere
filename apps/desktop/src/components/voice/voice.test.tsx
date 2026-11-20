import { describe, expect, it } from "vitest";
import { fireEvent, screen, waitFor, within } from "@solidjs/testing-library";
import { TextSelection } from "prosemirror-state";
import { setParagraph } from "../../test/editor-harness.ts";
import { renderApp, seedLocal, tick, type RenderedApp } from "../../test/app-harness.tsx";

async function withOpenPage(options: Parameters<typeof renderApp>[0] = {}, installModel = true) {
  const ui = await renderApp({
    setup: async (memory) => {
      await seedLocal(memory, ["Notes"]);
      if (installModel) await memory.api.voiceDownloadModel({ name: "small-q5" });
    },
    ...options,
  });
  const [page] = await ui.memory.api.treeSnapshot();
  await screen.findByRole("navigation", { name: "Workspace" });
  ui.app.openPage(page?.id as string);
  const editor = await waitFor(() => ui.app.ui.activeEditor() ?? Promise.reject(new Error("editor not ready")));
  return { ui, editor, view: editor.view };
}

const rejectionOf = async (promise: Promise<unknown>) => {
  try {
    await promise;
  } catch (error) {
    return error as { code: string };
  }
  throw new Error("expected rejection");
};

const openSettings = async (ui: RenderedApp) => {
  await ui.user.click(await screen.findByRole("button", { name: "Settings" }));
  return screen.findByRole("region", { name: "Settings" });
};

describe("hold to talk", () => {
  it("records while the hotkey is held and inserts the text at the cursor on release", async () => {
    const { ui, view } = await withOpenPage();
    setParagraph(view, "Say  now", 4);
    await tick(40);
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 5)));
    ui.memory.control.setNextTranscript("xin chào");

    ui.memory.control.pressHotkey(true);
    const indicator = await screen.findByText("Recording… release to insert");
    expect(indicator.closest('[role="status"]')).not.toBeNull();
    expect((await ui.memory.api.voiceStatus()).recording).toBe(true);

    ui.memory.control.pressHotkey(false);
    await waitFor(() => expect(view.state.doc.textContent).toBe("Say xin chào now"));
    expect(view.state.selection.from).toBe(5 + "xin chào".length);
    await waitFor(() => expect(screen.queryByText("Recording… release to insert")).toBeNull());
    expect((await ui.memory.api.voiceStatus()).recording).toBe(false);
  });

  it("replaces the selected text", async () => {
    const { ui, view } = await withOpenPage();
    setParagraph(view, "replace THIS word");
    await tick(40);
    view.dispatch(view.state.tr.setSelection(TextSelection.create(view.state.doc, 9, 13)));
    ui.memory.control.setNextTranscript("that");
    ui.memory.control.pressHotkey(true);
    await screen.findByText("Recording… release to insert");
    ui.memory.control.pressHotkey(false);
    await waitFor(() => expect(view.state.doc.textContent).toBe("replace that word"));
  });

  it("works from the on-screen button with the pointer", async () => {
    const { ui, view } = await withOpenPage();
    setParagraph(view, "");
    ui.memory.control.setNextTranscript("hello there");
    const button = await screen.findByRole("button", { name: "Hold to talk" });
    fireEvent.pointerDown(button);
    await screen.findByText("Recording… release to insert");
    expect(button.getAttribute("aria-pressed")).toBe("true");
    fireEvent.pointerUp(button);
    await waitFor(() => expect(view.state.doc.textContent).toBe("hello there"));
    expect(button.getAttribute("aria-pressed")).toBe("false");
  });

  it("works from the keyboard on the button", async () => {
    const { ui, view } = await withOpenPage();
    setParagraph(view, "");
    ui.memory.control.setNextTranscript("by keyboard");
    const button = await screen.findByRole("button", { name: "Hold to talk" });
    fireEvent.keyDown(button, { key: " " });
    await screen.findByText("Recording… release to insert");
    fireEvent.keyDown(button, { key: " ", repeat: true });
    fireEvent.keyUp(button, { key: " " });
    await waitFor(() => expect(view.state.doc.textContent).toBe("by keyboard"));
  });

  it("stops right away when released before recording had started", async () => {
    const { ui, view } = await withOpenPage();
    setParagraph(view, "");
    const start = ui.memory.api.voiceStart;
    ui.memory.api.voiceStart = async () => {
      await tick(60);
      return start();
    };
    ui.memory.control.setNextTranscript("quick tap");
    ui.memory.control.pressHotkey(true);
    await tick(10);
    ui.memory.control.pressHotkey(false);
    await waitFor(() => expect(view.state.doc.textContent).toBe("quick tap"), { timeout: 1500 });
    expect((await ui.memory.api.voiceStatus()).recording).toBe(false);
  });

  it("cancels with Escape and inserts nothing", async () => {
    const { ui, view } = await withOpenPage();
    setParagraph(view, "unchanged");
    ui.memory.control.setNextTranscript("should not appear");
    ui.memory.control.pressHotkey(true);
    await screen.findByText("Recording… release to insert");
    fireEvent.keyDown(window, { key: "Escape" });
    await waitFor(() => expect(screen.queryByText("Recording… release to insert")).toBeNull());
    ui.memory.control.pressHotkey(false);
    await tick(30);
    expect(view.state.doc.textContent).toBe("unchanged");
    expect((await ui.memory.api.voiceStatus()).recording).toBe(false);
  });

  it("puts the text after a selected divider instead of replacing it", async () => {
    const { ui, view } = await withOpenPage();
    const { schema } = view.state;
    view.dispatch(view.state.tr.replaceWith(0, view.state.doc.content.size, [schema.node("divider"), schema.node("paragraph")]));
    await tick(40);
    const { NodeSelection } = await import("prosemirror-state");
    view.dispatch(view.state.tr.setSelection(NodeSelection.create(view.state.doc, 0)));
    ui.memory.control.setNextTranscript("after the line");
    ui.memory.control.pressHotkey(true);
    await screen.findByText("Recording… release to insert");
    ui.memory.control.pressHotkey(false);
    await waitFor(() => expect(view.state.doc.textContent).toContain("after the line"));
    expect(view.state.doc.firstChild?.type.name).toBe("divider");
  });

  it("shows the text in a message when no page is open", async () => {
    const ui = await renderApp({
      setup: async (memory) => {
        await seedLocal(memory);
        await memory.api.voiceDownloadModel({ name: "small-q5" });
      },
    });
    await screen.findByRole("navigation", { name: "Workspace" });
    ui.memory.control.setNextTranscript("orphan text");
    ui.memory.control.pressHotkey(true);
    await screen.findByText("Recording… release to insert");
    ui.memory.control.pressHotkey(false);
    expect(await screen.findByText(/No page is open to receive the text: orphan text/)).toBeTruthy();
  });

  it("explains that a model is needed and does not get stuck recording", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    await screen.findByRole("navigation", { name: "Workspace" });
    ui.memory.control.pressHotkey(true);
    expect((await screen.findByRole("alert")).textContent).toContain("Download a transcription model first.");
    expect(screen.queryByText("Recording… release to insert")).toBeNull();
    expect((await ui.memory.api.voiceStatus()).recording).toBe(false);
  });
});

describe("voice settings", () => {
  it("hides the whole feature when the build has no transcription", async () => {
    const ui = await renderApp({ voiceAvailable: false, setup: async (memory) => void (await seedLocal(memory)) });
    await screen.findByRole("navigation", { name: "Workspace" });
    expect(screen.queryByRole("button", { name: "Hold to talk" })).toBeNull();
    const settings = await openSettings(ui);
    expect(within(settings).queryByText("Voice input")).toBeNull();
    ui.memory.control.pressHotkey(true);
    await tick(30);
    expect(screen.queryByText("Recording… release to insert")).toBeNull();
    expect((await ui.memory.api.voiceStatus()).recording).toBe(false);
  });

  it("downloads a model with visible progress and marks it as in use", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    const settings = await openSettings(ui);
    const row = (await within(settings).findByText("small-q5")).closest("li") as HTMLElement;
    expect(within(row).queryByText("In use")).toBeNull();
    await ui.user.click(within(row).getByRole("button", { name: "Download" }));
    await within(row).findByRole("progressbar");
    expect(await within(row).findByText("In use")).toBeTruthy();
    expect(within(row).queryByRole("progressbar")).toBeNull();
    expect((await ui.memory.api.voiceStatus()).models.find((m) => m.name === "small-q5")?.installed).toBe(true);

    await ui.user.click(within(row).getByRole("button", { name: "Delete" }));
    await waitFor(() => expect(within(row).getByRole("button", { name: "Download" })).toBeTruthy());
  });

  it("chooses the active model among the installed ones", async () => {
    const ui = await renderApp({
      setup: async (memory) => {
        await seedLocal(memory);
        await memory.api.voiceDownloadModel({ name: "small-q5" });
        await memory.api.voiceDownloadModel({ name: "large-v3-turbo-q5" });
      },
    });
    const settings = await openSettings(ui);
    const large = (await within(settings).findByText("large-v3-turbo-q5")).closest("li") as HTMLElement;
    await ui.user.click(within(large).getByRole("button", { name: "Use this model" }));
    await waitFor(async () => expect((await ui.memory.api.voiceStatus()).activeModel).toBe("large-v3-turbo-q5"));
    expect(await within(large).findByText("In use")).toBeTruthy();
    expect((await rejectionOf(ui.memory.api.voiceSelectModel({ name: "nope" }))).code).toBe("unknown_model");
  });

  it("shows a download failure", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    ui.memory.api.voiceDownloadModel = () => Promise.reject({ code: "network", message: "The download server cannot be reached." });
    const settings = await openSettings(ui);
    const row = (await within(settings).findByText("small-q5")).closest("li") as HTMLElement;
    await ui.user.click(within(row).getByRole("button", { name: "Download" }));
    expect((await screen.findByRole("alert")).textContent).toContain("cannot be reached");
    expect(within(row).getByRole("button", { name: "Download" })).toBeTruthy();
  });

  it("saves language, keep-audio and a captured hotkey", async () => {
    const ui = await renderApp({ setup: async (memory) => void (await seedLocal(memory)) });
    const settings = await openSettings(ui);
    const voice = within(settings.querySelector('[aria-labelledby="voice-settings-title"]') as HTMLElement);
    fireEvent.change(voice.getByLabelText("Language"), { target: { value: "vi" } });
    await waitFor(async () => expect((await ui.memory.api.settingsGet()).voiceLanguage).toBe("vi"));

    await ui.user.click(voice.getByRole("checkbox", { name: "Keep the recording as an attachment" }));
    await waitFor(async () => expect((await ui.memory.api.settingsGet()).voiceKeepAudio).toBe(true));

    const hotkey = voice.getByRole("button", { name: /Hold-to-talk hotkey/ });
    await ui.user.click(hotkey);
    expect(hotkey.textContent).toBe("Press the keys…");
    fireEvent.keyDown(document, { key: "k", code: "KeyK" });
    expect(hotkey.textContent).toBe("Press the keys…");
    fireEvent.keyDown(document, { key: "Control", code: "ControlLeft", ctrlKey: true });
    expect(hotkey.textContent).toBe("Press the keys…");
    fireEvent.keyDown(document, { key: "j", code: "KeyJ", ctrlKey: true, altKey: true });
    await waitFor(async () => expect((await ui.memory.api.settingsGet()).voiceHotkey).toBe("CmdOrCtrl+Alt+J"));
    expect(hotkey.textContent).toBe("Ctrl+Alt+J");

    await ui.user.click(hotkey);
    fireEvent.keyDown(document, { key: "Escape", code: "Escape" });
    expect(hotkey.textContent).toBe("Ctrl+Alt+J");
  });
});
