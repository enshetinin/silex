-- Source of truth for schema version 1. Applied transactionally by Rust.
CREATE TABLE pages (
    id TEXT PRIMARY KEY,
    parent_id TEXT REFERENCES pages(id) ON DELETE SET NULL,
    title TEXT NOT NULL DEFAULT 'Sin título',
    sort_order INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    deleted_at TEXT
);
CREATE INDEX pages_parent_sort_idx ON pages(parent_id, sort_order);

CREATE TABLE blocks (
    id TEXT PRIMARY KEY,
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    kind TEXT NOT NULL CHECK (kind IN ('paragraph', 'heading', 'todo')),
    content TEXT NOT NULL DEFAULT '',
    checked INTEGER NOT NULL DEFAULT 0 CHECK (checked IN (0, 1)),
    position INTEGER NOT NULL DEFAULT 0,
    created_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX blocks_page_position_idx ON blocks(page_id, position);

-- Page title and block text full-text indexes, synchronized by triggers.
CREATE VIRTUAL TABLE page_search USING fts5(title, content='pages', content_rowid='rowid', tokenize='unicode61');
CREATE VIRTUAL TABLE block_search USING fts5(content, content='blocks', content_rowid='rowid', tokenize='unicode61');

CREATE TRIGGER pages_search_ai AFTER INSERT ON pages BEGIN
    INSERT INTO page_search(rowid, title) VALUES (new.rowid, new.title);
END;
CREATE TRIGGER pages_search_ad AFTER DELETE ON pages BEGIN
    INSERT INTO page_search(page_search, rowid, title) VALUES ('delete', old.rowid, old.title);
END;
CREATE TRIGGER pages_search_au AFTER UPDATE OF title ON pages BEGIN
    INSERT INTO page_search(page_search, rowid, title) VALUES ('delete', old.rowid, old.title);
    INSERT INTO page_search(rowid, title) VALUES (new.rowid, new.title);
END;

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
