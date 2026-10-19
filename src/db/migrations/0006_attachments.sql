-- Schema version 6: local attachments (docs/decisions/0007-attachments.md).
-- Applied transactionally by Rust, after a verified copy of the v5 file.
--
-- Files live inside SQLite as BLOBs, at most 20 MB each, so backups,
-- restores and migrations include them with no extra code.
CREATE TABLE attachments (
    id         TEXT PRIMARY KEY,
    name       TEXT NOT NULL,              -- original name: shown and exported, never a path
    mime       TEXT NOT NULL,              -- from the content's signature, else 'application/octet-stream'
    size       INTEGER NOT NULL CHECK (size BETWEEN 0 AND 20971520),
    width      INTEGER CHECK (width IS NULL OR width > 0),     -- images: read when added
    height     INTEGER CHECK (height IS NULL OR height > 0),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    data       BLOB NOT NULL CHECK (length(data) = size)       -- last: never read by listings
);

-- A new block kind, `file`, needs a new CHECK: SQLite cannot alter one, so
-- `blocks` is rebuilt as in 0003. Rowids are copied explicitly so the
-- external-content FTS index (keyed by rowid) stays valid; it is rebuilt at
-- the end anyway. A file block's content is the name shown (and searched).
CREATE TABLE blocks_v6 (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'paragraph', 'heading_1', 'heading_2', 'heading_3',
        'todo', 'bulleted', 'numbered', 'quote', 'code', 'file'
    )),
    content TEXT NOT NULL DEFAULT '',
    checked INTEGER NOT NULL DEFAULT 0 CHECK (checked IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    indent INTEGER NOT NULL DEFAULT 0 CHECK (indent BETWEEN 0 AND 6),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- An attachment in use cannot be deleted.
    attachment_id TEXT REFERENCES attachments(id) ON DELETE RESTRICT,
    CHECK ((kind = 'file') = (attachment_id IS NOT NULL))
);

INSERT INTO blocks_v6 (rowid, id, page_id, kind, content, checked, position, indent, created_at, updated_at)
SELECT rowid, id, page_id, kind, content, checked, position, indent, created_at, updated_at
FROM blocks;

-- Dropping the old table also drops its index and FTS triggers.
DROP TABLE blocks;
ALTER TABLE blocks_v6 RENAME TO blocks;
CREATE INDEX blocks_page_position_idx ON blocks(page_id, position);
CREATE INDEX blocks_attachment_idx ON blocks(attachment_id) WHERE attachment_id IS NOT NULL;

CREATE TRIGGER blocks_search_ai AFTER INSERT ON blocks BEGIN
    INSERT INTO block_search(rowid, content) VALUES (new.rowid, new.content);
END;
CREATE TRIGGER blocks_search_ad AFTER DELETE ON blocks BEGIN
    INSERT INTO block_search(block_search, rowid, content) VALUES ('delete', old.rowid, old.content);
END;
CREATE TRIGGER blocks_search_au AFTER UPDATE OF content ON blocks BEGIN
    INSERT INTO block_search(block_search, rowid, content) VALUES ('delete', old.rowid, old.content);
    INSERT INTO block_search(rowid, content) VALUES (new.rowid, new.content);
END;

INSERT INTO block_search(block_search) VALUES ('rebuild');
