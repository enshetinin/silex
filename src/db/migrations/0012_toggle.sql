-- Schema version 12: toggle blocks (docs/decisions/0016-toggle-blocks.md).
-- Applied transactionally by Rust, after a verified copy of the v11 file.
--
-- A toggle is a text block (with formatting) whose content is the blocks
-- after it with more indent. Whether it is folded is view state, kept in
-- `settings`, not here.
--
-- The new kind needs a new CHECK, so `blocks` is rebuilt exactly as in
-- 0011: rowids are copied explicitly (the external-content FTS index is
-- keyed by them), every column with them, and the index is rebuilt at the
-- end anyway.
CREATE TABLE blocks_v12 (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'paragraph', 'heading_1', 'heading_2', 'heading_3',
        'todo', 'bulleted', 'numbered', 'quote', 'code', 'file', 'page_link',
        'divider', 'callout', 'toggle'
    )),
    content TEXT NOT NULL DEFAULT '',
    checked INTEGER NOT NULL DEFAULT 0 CHECK (checked IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    indent INTEGER NOT NULL DEFAULT 0 CHECK (indent BETWEEN 0 AND 6),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- An attachment in use cannot be deleted.
    attachment_id TEXT REFERENCES attachments(id) ON DELETE RESTRICT,
    -- The page a link points at (see 0007).
    link_page_id TEXT REFERENCES pages(id) ON DELETE SET NULL,
    marks TEXT CHECK (marks IS NULL OR json_valid(marks)),
    CHECK ((kind = 'file') = (attachment_id IS NOT NULL)),
    CHECK (kind = 'page_link' OR link_page_id IS NULL),
    -- A divider has no text to search, export or format.
    CHECK (kind <> 'divider' OR (content = '' AND marks IS NULL))
);

INSERT INTO blocks_v12 (rowid, id, page_id, kind, content, checked, position, indent,
                        created_at, updated_at, attachment_id, link_page_id, marks)
SELECT rowid, id, page_id, kind, content, checked, position, indent, created_at, updated_at,
       attachment_id, link_page_id, marks
FROM blocks;

-- Dropping the old table also drops its indexes and FTS triggers.
DROP TABLE blocks;
ALTER TABLE blocks_v12 RENAME TO blocks;
CREATE INDEX blocks_page_position_idx ON blocks(page_id, position);
CREATE INDEX blocks_attachment_idx ON blocks(attachment_id) WHERE attachment_id IS NOT NULL;
CREATE INDEX blocks_link_idx ON blocks(link_page_id) WHERE link_page_id IS NOT NULL;

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
