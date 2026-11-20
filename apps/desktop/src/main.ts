import { invoke } from "@tauri-apps/api/core";

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

window.addEventListener("DOMContentLoaded", () => {
  void showVersion();
});
