-- Schema version 2: databases, synced blocks, reminders.
--
-- Everything added here is derived from CRDT documents (and can be rebuilt with
-- `Workspace::rebuild_indexes`) except `reminder_state`, which is device state.

-- `nodes` gains the `database` kind, a sort key for titles and a flag for
-- database rows (kept out of the sidebar). SQLite cannot change a CHECK
-- constraint, so the table is rebuilt; `rid` values are kept because the
-- search index and `page_text` refer to them.
CREATE TABLE nodes_new (
    rid         INTEGER PRIMARY KEY AUTOINCREMENT,
    id          TEXT NOT NULL UNIQUE,
    parent_id   TEXT,
    position    INTEGER NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('folder', 'page', 'database')),
    title       TEXT NOT NULL,
    title_norm  TEXT NOT NULL DEFAULT '',
    title_key   BLOB,
    icon        TEXT,
    favorite    INTEGER NOT NULL DEFAULT 0,
    created_at  INTEGER NOT NULL,
    updated_at  INTEGER NOT NULL,
    trashed_at  INTEGER,
    hidden      INTEGER NOT NULL DEFAULT 0,
    in_database INTEGER NOT NULL DEFAULT 0
);
INSERT INTO nodes_new (rid, id, parent_id, position, kind, title, title_norm, icon, favorite,
                       created_at, updated_at, trashed_at, hidden)
SELECT rid, id, parent_id, position, kind, title, title_norm, icon, favorite,
       created_at, updated_at, trashed_at, hidden
FROM nodes;
DROP TABLE nodes;
ALTER TABLE nodes_new RENAME TO nodes;
CREATE INDEX nodes_by_parent ON nodes (parent_id, position);
CREATE INDEX nodes_by_visibility ON nodes (hidden, parent_id, position);

-- What a stored document is, besides a plain page: `database`, `synced`,
-- `library` or `reminders`. All of them are addressed by a node-shaped id and
-- travel through sync as `page` documents.
ALTER TABLE docs ADD COLUMN role TEXT NOT NULL DEFAULT 'page';

-- Derived: one row per stored cell value, already converted to the property's
-- current type. `num` holds numbers, checkboxes (0/1) and select ranks, `txt`
-- folded text or an ISO date start, `txt2` a date end, `opt` an option id
-- (one row per option for multi-select, numbered by `ord`), `skey` the
-- collation key of text. Relations live in `db_links`.
CREATE TABLE db_cells (
    db_id   TEXT NOT NULL,
    prop_id TEXT NOT NULL,
    row_id  TEXT NOT NULL,
    ord     INTEGER NOT NULL DEFAULT 0,
    num     REAL,
    txt     TEXT,
    txt2    TEXT,
    opt     TEXT,
    skey    BLOB,
    PRIMARY KEY (db_id, prop_id, row_id, ord)
) WITHOUT ROWID;
CREATE INDEX db_cells_by_row ON db_cells (row_id);

-- Derived: the effective values of a row as JSON, and the text of its
-- properties for search.
CREATE TABLE db_rows (
    row_id      TEXT PRIMARY KEY NOT NULL,
    db_id       TEXT NOT NULL,
    vals        TEXT NOT NULL,
    search_raw  TEXT NOT NULL DEFAULT '',
    search_norm TEXT NOT NULL DEFAULT ''
) WITHOUT ROWID;
CREATE INDEX db_rows_by_db ON db_rows (db_id);

-- Derived: links of forward relation properties.
CREATE TABLE db_links (
    prop_id TEXT NOT NULL,
    src_row TEXT NOT NULL,
    dst_row TEXT NOT NULL,
    PRIMARY KEY (prop_id, src_row, dst_row)
) WITHOUT ROWID;
CREATE INDEX db_links_by_dst ON db_links (prop_id, dst_row);
CREATE INDEX db_links_by_src ON db_links (src_row);

-- Derived: which pages show which synced block, and since when a synced
-- block has had no occurrence at all.
CREATE TABLE synced_refs (
    sync_id TEXT NOT NULL,
    page_id TEXT NOT NULL,
    PRIMARY KEY (sync_id, page_id)
) WITHOUT ROWID;
CREATE INDEX synced_refs_by_page ON synced_refs (page_id);

CREATE TABLE synced_state (
    sync_id     TEXT PRIMARY KEY NOT NULL,
    orphaned_at INTEGER NOT NULL
) WITHOUT ROWID;

-- Device state: which reminder moments this device already fired or decided
-- were too old to fire. Not synced on purpose, so each device fires once.
CREATE TABLE reminder_state (
    reminder_id TEXT NOT NULL,
    due_at      INTEGER NOT NULL,
    outcome     TEXT NOT NULL CHECK (outcome IN ('fired', 'skipped')),
    decided_at  INTEGER NOT NULL,
    PRIMARY KEY (reminder_id, due_at)
) WITHOUT ROWID;
