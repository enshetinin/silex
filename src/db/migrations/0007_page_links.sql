-- Schema version 7: links between pages (docs/decisions/0008-page-links.md).
-- Applied transactionally by Rust, after a verified copy of the v6 file.
--
-- A link is a block of its own, `page_link`, pointing at its page by id: it
-- never breaks when the page is renamed or moved. Its content is the page's
-- title, kept up to date by Rust, for search and export.
--
-- The new kind needs a new CHECK, so `blocks` is rebuilt as in 0003 and
-- 0006: rowids are copied explicitly (the external-content FTS index is
-- keyed by them) and the index is rebuilt at the end anyway.
CREATE TABLE blocks_v7 (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN (
        'paragraph', 'heading_1', 'heading_2', 'heading_3',
        'todo', 'bulleted', 'numbered', 'quote', 'code', 'file', 'page_link'
    )),
    content TEXT NOT NULL DEFAULT '',
    checked INTEGER NOT NULL DEFAULT 0 CHECK (checked IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    indent INTEGER NOT NULL DEFAULT 0 CHECK (indent BETWEEN 0 AND 6),
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    -- An attachment in use cannot be deleted.
    attachment_id TEXT REFERENCES attachments(id) ON DELETE RESTRICT,
    -- The page a link points at. Pages are never deleted for real (trash):
    -- SET NULL is a safety net, read as «a page that no longer exists»
    -- (so a link may lose its page, but only a link has one).
    link_page_id TEXT REFERENCES pages(id) ON DELETE SET NULL,
    CHECK ((kind = 'file') = (attachment_id IS NOT NULL)),
    CHECK (kind = 'page_link' OR link_page_id IS NULL)
);

INSERT INTO blocks_v7 (rowid, id, page_id, kind, content, checked, position, indent, created_at,
                       updated_at, attachment_id)
SELECT rowid, id, page_id, kind, content, checked, position, indent, created_at, updated_at,
       attachment_id
FROM blocks;

-- Dropping the old table also drops its index and FTS triggers.
DROP TABLE blocks;
ALTER TABLE blocks_v7 RENAME TO blocks;
CREATE INDEX blocks_page_position_idx ON blocks(page_id, position);
CREATE INDEX blocks_attachment_idx ON blocks(attachment_id) WHERE attachment_id IS NOT NULL;
-- Backlinks: the blocks that point at a page.
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
