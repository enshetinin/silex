-- Schema version 8: page history (docs/decisions/0009-page-history.md).
-- Applied transactionally by Rust, after a verified copy of the v7 file.
--
-- A version is a whole snapshot of a page: its title and its blocks as a
-- JSON list (`format` 1, written and read by `src/versions.rs`). Rust saves
-- one when a page is saved and its last version is older than 10 minutes,
-- and thins them out in the same transaction.
--
-- Only a new table: nothing is rebuilt. Pages are never deleted for real,
-- so the cascade is only a safety net. Versions are not searched (no FTS).
CREATE TABLE page_versions (
    id         INTEGER PRIMARY KEY,
    page_id    TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    title      TEXT NOT NULL,
    format     INTEGER NOT NULL DEFAULT 1,
    blocks     TEXT NOT NULL CHECK (json_valid(blocks)),
    size       INTEGER NOT NULL CHECK (size = octet_length(blocks))
);

CREATE INDEX page_versions_page_idx ON page_versions(page_id, created_at);
