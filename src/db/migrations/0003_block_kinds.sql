-- Schema version 3: typed block kinds (headings 1-3, lists, quote, code) and indent.
-- See docs/decisions/0003-block-model.md. Applied transactionally by Rust.
--
-- SQLite cannot alter a CHECK constraint, so `blocks` is rebuilt. Rowids are
-- copied explicitly so the external-content FTS index (keyed by rowid) stays
-- valid; it is rebuilt at the end anyway.
CREATE TABLE blocks_v3 (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'paragraph', 'heading_1', 'heading_2', 'heading_3',
        'todo', 'bulleted', 'numbered', 'quote', 'code'
    )),
    content TEXT NOT NULL DEFAULT '',
    checked INTEGER NOT NULL DEFAULT 0 CHECK (checked IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    indent INTEGER NOT NULL DEFAULT 0 CHECK (indent BETWEEN 0 AND 6),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO blocks_v3 (rowid, id, page_id, kind, content, checked, position, indent, created_at, updated_at)
SELECT rowid, id, page_id,
       CASE kind WHEN 'heading' THEN 'heading_1' ELSE kind END,
       content, checked, position, 0, created_at, updated_at
FROM blocks;

-- Dropping the old table also drops its index and FTS triggers.
DROP TABLE blocks;
ALTER TABLE blocks_v3 RENAME TO blocks;
CREATE INDEX blocks_page_position_idx ON blocks(page_id, position);

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
