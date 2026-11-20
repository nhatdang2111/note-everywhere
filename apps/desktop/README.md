# Note Everywhere desktop UI

The webview side of the desktop app: a Solid + TypeScript UI that talks to the Rust core only through the contract in `src/api/contract.ts` (`CoreApi` for commands, `CoreEvents` for events). The Rust side lives in `src-tauri/` and is not part of this package's UI code.

Design rules (tokens, type, motion, accessibility) are in `docs/design-guidelines.md`.

## Layout

```
src/
  api/
    contract.ts       the UI <-> core contract (authoritative, do not edit from the UI)
    tauri.ts          real implementation over @tauri-apps/api (invoke + listen)
    index.ts          picks the implementation by detecting the Tauri runtime
    memory*.ts        in-memory implementation: tests and browser preview only
    preview-seed.ts   sample pages for `?demo` in preview mode
  editor/             ProseMirror + Loro page editor (plain TypeScript, no framework)
  state/              a few stores (session, tree, tabs, vault, voice, settings, sync, ui)
  components/         auth, shell (sidebar, tree, tabs), editor UI, search, vault, voice, settings
  i18n/               English catalogue (source of keys) and Vietnamese
  styles/             tokens.css (design tokens) and the stylesheets that use them
  test/               test setup and harnesses
spikes/               headless scripts that back docs/decisions/0006 (they import src/editor)
```

The editor modules under `src/editor` must stay loadable by plain Node (no JSX, no CSS imports, no Solid): the spike scripts run them without a bundler.

## Run

```
pnpm install
pnpm dev            # http://localhost:1420
pnpm typecheck
pnpm test
```

Inside Tauri (`pnpm tauri dev`) the UI uses the real core. In a plain browser, `pnpm dev` starts **preview mode**: an in-memory core, so the whole UI can be tried without Rust. A banner says so, and data disappears on reload. Add `?demo` to the URL to start with a local workspace and a few synthetic pages.

Preview-mode stand-ins worth knowing about:

- Account: the setup code is `preview-setup-code`; passwords need 10+ characters; recovery keys are random and synthetic.
- Vault: the re-authentication grace period is 60 s, the clipboard clears after 30 s, idle lock follows Settings.
- Voice: models "download" in a few steps and every recording returns a fixed sentence. Nothing is transcribed.
- Files: Markdown import and CSV import use a browser file input; export downloads one `.md` file.

The in-memory core is only reachable when `import.meta.env.DEV` is true, so a production bundle neither contains nor can select it.

## Tests

Vitest on jsdom, with `@solidjs/testing-library` for components. Everything runs against the in-memory core.

```
pnpm test                          # all
pnpm exec vitest run src/editor    # one folder
```

What is covered: the tree model and drag-and-drop planning (including refusing a drop into a node's own descendant), tabs and history, the editor (input rules, slash and page-link triggers, turn-into and block moves keeping content, Markdown/HTML/image paste, find, remote updates keeping the caret, undo limited to local edits), the in-memory core itself, the Tauri bridge with mocked `invoke`/`listen`, and the screens: sign-up with recovery key, sign-in again, workspace, trash, quick search, vault, voice, settings, import/export.

jsdom has no layout, so pointer-driven block dragging (which needs element geometry) is covered through the move commands it calls, not through real drag events.

## Document conventions the core must know

These are choices made in the UI that the page document carries (see `docs/decisions/0006-page-document-structure.md` for the base structure):

- An inline link to another page is a `link` mark whose `href` is `ne-page:<pageId>`.
- An image block's `src` is `attachment:<hash>`; the UI resolves it to a URL with `attachmentUrl` when it displays the image.
- The attributes remembered when a block changes kind (for example a to-do's checked state while it is a heading) live in the open editor, not in the document.
