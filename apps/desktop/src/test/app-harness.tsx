import { render } from "@solidjs/testing-library";
import userEvent from "@testing-library/user-event";
import type { JSX } from "solid-js";
import { Root } from "../app.tsx";
import type { Core } from "../api/index.ts";
import { createMemoryCore, MEMORY_SETUP_CODE, type MemoryCore, type MemoryOptions } from "../api/memory.ts";
import { AppProvider, useApp, type App } from "../state/app.tsx";

export interface RenderedApp {
  user: ReturnType<typeof userEvent.setup>;
  memory: MemoryCore;
  app: App;
  container: HTMLElement;
  /** Everything rendered through portals (dialogs, menus, toasts) lives under `document.body`. */
  body: HTMLElement;
  unmount(): void;
}

export const GOOD_PASSWORD = "correct horse battery";
export const SERVER_URL = "https://notes.example.test";

/** Captures the app object from inside the provider so tests can reach stores and the open editor. */
function Capture(props: { onApp(app: App): void }): JSX.Element {
  props.onApp(useApp());
  return <></>;
}

export interface RenderOptions extends MemoryOptions {
  /** Prepare the in-memory core before the UI mounts, for example by creating pages. */
  setup?(memory: MemoryCore): Promise<void>;
}

/** Mount the whole app on an in-memory core. */
export async function renderApp(options: RenderOptions = {}): Promise<RenderedApp> {
  const { setup, ...memoryOptions } = options;
  const memory = createMemoryCore({ syncDelayMs: 0, ...memoryOptions });
  await setup?.(memory);
  const core: Core = { api: memory.api, events: memory.events, preview: false };
  let captured: App | undefined;
  const view = render(() => (
    <AppProvider core={core}>
      <Capture onApp={(app) => (captured = app)} />
      <Root />
    </AppProvider>
  ));
  if (!captured) throw new Error("app did not mount");
  return { user: userEvent.setup(), memory, app: captured, container: view.container, body: document.body, unmount: view.unmount };
}

/** A workspace that is already open, with the given pages at the top level. */
export async function seedLocal(memory: MemoryCore, titles: string[] = []): Promise<string[]> {
  await memory.api.workspaceCreateLocal();
  const ids: string[] = [];
  for (const title of titles) ids.push(await memory.api.treeCreate({ kind: "page", parentId: null, title }));
  return ids;
}

export async function seedSignedIn(memory: MemoryCore, titles: string[] = []): Promise<string[]> {
  await memory.api.signUp({ serverUrl: SERVER_URL, setupCode: MEMORY_SETUP_CODE, username: "ada", password: GOOD_PASSWORD, deviceLabel: "Laptop" });
  const ids: string[] = [];
  for (const title of titles) ids.push(await memory.api.treeCreate({ kind: "page", parentId: null, title }));
  return ids;
}

export const tick = (ms = 0): Promise<void> => new Promise((resolve) => setTimeout(resolve, ms));
