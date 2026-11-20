-- Schema version 1.
--
-- `docs` and `updates` are the source of truth for every CRDT document. The
-- tables marked "Derived" are computed from them and can be recomputed at any
-- time; `attachments`, `outbox`, `sync_meta` and `settings` hold their own data.

-- One row per CRDT document: the workspace tree, each page, each vault doc.
-- `snapshot` is the last compacted state; `updates` holds what came after.
-- `version` is the encoded version vector of snapshot plus all updates.
CREATE TABLE docs (
    id           TEXT PRIMARY KEY NOT NULL,
    kind         TEXT NOT NULL CHECK (kind IN ('tree', 'page', 'vault')),
    snapshot     BLOB NOT NULL,
    version      BLOB NOT NULL,
    update_count INTEGER NOT NULL DEFAULT 0,
    update_bytes INTEGER NOT NULL DEFAULT 0,
    updated_at   INTEGER NOT NULL
) WITHOUT ROWID;

CREATE TABLE updates (
    seq    INTEGER PRIMARY KEY AUTOINCREMENT,
    doc_id TEXT NOT NULL REFERENCES docs (id) ON DELETE CASCADE,
    bytes  BLOB NOT NULL
);
CREATE INDEX updates_by_doc ON updates (doc_id, seq);

-- Derived: a mirror of the workspace tree for sidebar queries. `position` is
-- the index among all siblings (trashed ones included); `hidden` is set when
-- the node or any ancestor is in the trash.
CREATE TABLE nodes (
    rid        INTEGER PRIMARY KEY AUTOINCREMENT,
    id         TEXT NOT NULL UNIQUE,
    parent_id  TEXT,
    position   INTEGER NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('folder', 'page')),
    title      TEXT NOT NULL,
    title_norm TEXT NOT NULL DEFAULT '',
    icon       TEXT,
    favorite   INTEGER NOT NULL DEFAULT 0,
    created_at INTEGER NOT NULL,
    updated_at INTEGER NOT NULL,
    trashed_at INTEGER,
    hidden     INTEGER NOT NULL DEFAULT 0
);
CREATE INDEX nodes_by_parent ON nodes (parent_id, position);
CREATE INDEX nodes_by_visibility ON nodes (hidden, parent_id, position);

-- Derived: plain body text of each page, kept to produce search snippets,
-- and its folded form (see `search`), which answers terms the trigram index
-- cannot and saves folding again when a title changes.
CREATE TABLE page_text (
    rid        INTEGER PRIMARY KEY NOT NULL,
    body       TEXT NOT NULL,
    body_norm  TEXT NOT NULL,
    word_count INTEGER NOT NULL
);

-- Derived: page links for backlinks. `kind`: 0 child page, 1 page link block,
-- 2 inline link.
CREATE TABLE links (
    source_id TEXT NOT NULL,
    target_id TEXT NOT NULL,
    kind      INTEGER NOT NULL,
    PRIMARY KEY (source_id, target_id, kind)
) WITHOUT ROWID;
CREATE INDEX links_by_target ON links (target_id);

-- Derived: full-text index over titles and bodies, folded by the application
-- (lowercase, no diacritics) before indexing. Trigrams give substring and
-- prefix matching; rows are keyed by `nodes.rid`. Contentless: the text lives
-- in `nodes` and `page_text`.
CREATE VIRTUAL TABLE search USING fts5 (
    title,
    body,
    content = '',
    contentless_delete = 1,
    tokenize = 'trigram'
);

-- Attachment blobs live on disk beside the database, named by content hash.
CREATE TABLE attachments (
    hash       TEXT PRIMARY KEY NOT NULL,
    file_name  TEXT NOT NULL,
    size       INTEGER NOT NULL,
    created_at INTEGER NOT NULL,
    ref_count  INTEGER NOT NULL DEFAULT 0
) WITHOUT ROWID;

-- Derived: which attachments each page shows.
CREATE TABLE attachment_refs (
    page_id TEXT NOT NULL,
    hash    TEXT NOT NULL,
    PRIMARY KEY (page_id, hash)
) WITHOUT ROWID;
CREATE INDEX attachment_refs_by_hash ON attachment_refs (hash);

-- Updates waiting to be sent to the sync server, oldest first.
CREATE TABLE outbox (
    seq        INTEGER PRIMARY KEY AUTOINCREMENT,
    client_id  TEXT NOT NULL UNIQUE,
    doc_id     TEXT NOT NULL,
    kind       TEXT NOT NULL CHECK (kind IN ('tree', 'page', 'vault')),
    bytes      BLOB NOT NULL,
    created_at INTEGER NOT NULL
);

-- Sync cursor and other sync bookkeeping.
CREATE TABLE sync_meta (
    key   TEXT PRIMARY KEY NOT NULL,
    value BLOB NOT NULL
) WITHOUT ROWID;

CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
) WITHOUT ROWID;
