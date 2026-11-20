import { invoke } from "@tauri-apps/api/core";
import { BLOCK_NODE_NAMES, createPageEditor, insertBlock, setHeading, toggleBold, toggleCode, toggleItalic, toggleStrike } from "./editor/index.ts";
import type { BlockKind } from "./editor/index.ts";

async function showVersion(): Promise<void> {
  const status = document.querySelector<HTMLParagraphElement>("#status");
  if (!status) return;
  try {
    const version = await invoke<string>("app_version");
    status.textContent = `Core ready, version ${version}`;
  } catch (error) {
    status.textContent = `Could not reach the Rust core: ${String(error)}`;
  }
}

function mountEditor(): void {
  const mount = document.querySelector<HTMLDivElement>("#editor");
  const toolbar = document.querySelector<HTMLDivElement>("#toolbar");
  const updates = document.querySelector<HTMLSpanElement>("#update-info");
  if (!mount || !toolbar || !updates) return;

  let bytes = 0;
  const editor = createPageEditor(mount, {
    onLocalUpdate: (update) => {
      // Placeholder for the IPC bridge: these bytes are what the Rust core imports.
      bytes += update.length;
      updates.textContent = `${bytes} bytes of Loro updates produced`;
    },
  });

  const addButton = (label: string, run: () => void) => {
    const button = document.createElement("button");
    button.type = "button";
    button.textContent = label;
    // Keep the editor selection: do not let the click steal focus.
    button.addEventListener("mousedown", (event) => event.preventDefault());
    button.addEventListener("click", () => {
      run();
      editor.view.focus();
    });
    toolbar.append(button);
  };

  const { view } = editor;
  addButton("Bold", () => toggleBold(view.state, view.dispatch));
  addButton("Italic", () => toggleItalic(view.state, view.dispatch));
  addButton("Code", () => toggleCode(view.state, view.dispatch));
  addButton("Strike", () => toggleStrike(view.state, view.dispatch));
  for (const level of [1, 2, 3] as const) {
    addButton(`H${level}`, () => setHeading(level)(view.state, view.dispatch));
  }
  for (const kind of BLOCK_NODE_NAMES.filter((name) => name !== "paragraph" && name !== "heading")) {
    addButton(kind.replace("_", " "), () => insertBlock(kind as BlockKind)(view.state, view.dispatch));
  }
}

// Module scripts run after the document is parsed, so no DOMContentLoaded
// listener is needed. Waiting for one would also race loro-crdt's async
// WebAssembly initialisation.
void showVersion();
mountEditor();
