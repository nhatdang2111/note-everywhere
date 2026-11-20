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
  components/         auth, shell (sidebar, tree, tabs), editor UI, search, vault, voice, settings,
                      database (table, board, list, calendar, editors), history, reminders
  lib/db/             pure logic behind the database UI: filter builder model, virtual window, board and
                      calendar reducers, cell conversion, Vietnamese collation (loadable by plain Node)
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
- Files: Markdown import and CSV import use a browser file input; export downloads one `.md` file. The Markdown & CSV export folder import uses a folder input and a simplified reading of the export (see below).
- Databases, templates, history, synced blocks, reminders and updates are all real behaviour in the in-memory core: queries filter, sort (Vietnamese alphabet) and group; type changes convert on read; history groups real Loro changes; reminders fire from a timer. `MemoryControl` has levers for tests (`seedRows`, `advanceClock`, `receiveRemoteCell`, `setHistoryTrimmed`, `announceUpdate`).

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

## Databases, templates, history, synced blocks, reminders, updates

- **Databases.** A database opens in the main area like a page (`components/database/`). `state/database-store.ts` is the controller of one open database: schema, the active view's rows, every edit. Table and list hold only pages of 200 rows near the viewport (`lib/db/virtual-window.ts`); the board holds 100 cards per column and loads more on request; the calendar queries the visible month. A `database:changed` event, or a `tree:changed` that names the database or a loaded row, re-reads the data in place: rows, columns and cards are drawn by id, so an open cell editor keeps its text and focus. Grouping applies to board views (table and list ignore a saved group).
- **Rows are pages the tree does not list.** The contract has no command that says which database a row belongs to, so `state/rows.ts` remembers rows the UI has shown (last 400, in local storage) and tabs carry `databaseId`. A row opened from search is traced to its database only when exactly one database matches the last breadcrumb title; otherwise it opens without its properties. Row values on a row page are found with a title query (`state/row-values.ts`). A `treeGet`/`dbRowGet` command would remove both workarounds.
- **Keyboard in the table.** Arrows, Home/End, Page Up/Down, Ctrl/Cmd with arrows for the edges; Enter or F2 edits, typing starts editing, Space toggles a checkbox, Delete clears, Escape cancels; Shift+Enter or Ctrl/Cmd+Enter opens the row; Menu or Shift+F10 opens the row menu. Copy and paste go through the browser's `copy`/`paste` events: text for any cell, and the exact value (`application/x-ne-cell`) between cells of one type. On the board, Alt+arrows move a card; column widths resize with the arrow keys on their handle.
- **Templates.** Page templates: "Save as template…" (top bar and tree menu) and "New page from template…" (sidebar menu, tree menu). Row templates: the split "New row" button and "Manage templates…" in each database.
- **History.** The clock button in the top bar opens the panel. `setRecordTimestamp(true)` and `setChangeMergeInterval(30)` are applied to every Loro document an editor binds to (`configureDoc` in `editor/binding.ts`, called by `bindingPlugins`, so it covers pages and shared documents alike).
- **Structured import.** "Import Markdown & CSV export folder…" in the sidebar menu and the tree menu. The in-memory version reads a picked folder: folders, `.md` pages, `.csv` databases (first column is the title, number and ISO-date columns are typed), images, links between pages, and lists what it skipped. The real reading is the core's.
- **Synced blocks.** `synced_block` is an atom in the editor schema (`syncId` attribute, no children). Its node view (`editor/synced-view.ts`) frames a nested editor bound to its own Loro document for the shared content; `state/synced-hub.ts` connects every open view to the core and keeps views of the same content equal. Block menu: "Create synced block" (confirms, works on the top-level blocks the selection touches), and on the block's own menu: copy reference (`ne-synced:<id>`, pasted into another page), pages that show it, unsync. Slash menu: "Synced block" inserts an existing one with `syncedInsert`. The nested editor refuses any content that would put a synced block inside shared content. The nested editor has no slash menu or block menu of its own.
- **Reminders.** The bell in the top bar sets a reminder on the page; the bell next to a date property of a row follows that date. The "Reminders" panel (sidebar) lists open reminders with Done, Snooze and Delete. When `reminder:due` arrives with `nativeShown: false` the UI shows a toast (with "Open page") and a webview notification if the system allows one; with `nativeShown: true` it shows nothing more.
- **Updates.** Settings has an Updates section (current version, state, release notes, Check, Download with progress, Install and restart after a confirmation) and the top bar shows a small pill while an update waits. When the state is `unavailable` only the reason is shown. Nothing downloads or installs without a click.
- **Voice recordings.** When `voiceStop` returns `audio` the page gets a paragraph after the text holding a link to `attachment:<hash>` ("Voice recording"); clicking it plays the file. The document has no file block, and a link mark is something every reader already understands.

## What the tests cover (added)

Table windowing at 10,000 rows, every cell editor, keyboard navigation, copy and paste, an in-progress edit surviving remote changes, filter builder serialisation both ways, sort/group/properties, board and calendar drag reducers and the DOM drag, type-change confirmation with a preview, options, relation picker, row pages, templates, history preview and restore (and the trimmed case), structured import report, synced block editing reflected in a second open page and in the core, the no-nesting rule, reminder dialog/panel/toast and its suppression, the updater states, voice attachment, the Loro timestamp settings, and the bridge's command names.
