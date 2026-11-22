CREATE TABLE db_properties_v16 (
    id          TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN (
        'text', 'number', 'checkbox', 'date', 'select',
        'multi_select', 'url', 'created', 'edited', 'relation'
    )),
    position    INTEGER NOT NULL,
    deleted_at  TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO db_properties_v16 (rowid, id, database_id, name, kind, position, deleted_at,
                               created_at, updated_at)
SELECT rowid, id, database_id, name, kind, position, deleted_at, created_at, updated_at
FROM db_properties;

DROP TABLE db_properties;
ALTER TABLE db_properties_v16 RENAME TO db_properties;
CREATE INDEX db_properties_database_idx ON db_properties(database_id, position);


-- Relation definitions and values use real page references, never copied titles.
CREATE TABLE db_relations (
    property_id TEXT PRIMARY KEY REFERENCES db_properties(id) ON DELETE CASCADE,
    target_database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE
);
CREATE TABLE db_relation_values (
    page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL REFERENCES db_properties(id) ON DELETE CASCADE,
    target_page_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    PRIMARY KEY (page_id, property_id, target_page_id)
);
CREATE INDEX db_relation_targets ON db_relation_values(target_page_id);
-- Retain settings when a block is removed so document undo can restore its view.
CREATE TABLE db_linked_views (
    block_id TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    config TEXT NOT NULL DEFAULT '{}'
);
