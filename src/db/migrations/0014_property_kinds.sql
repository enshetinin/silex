-- Schema version 14: multi-select properties (H13.2, ADR 0018), and the
-- kinds of H13.3 (url, created, edited) in the same CHECK.
-- Applied transactionally by Rust, after a verified copy of the v13 file,
-- with foreign keys off: `db_select_options` and `db_values` point at
-- `db_properties` with ON DELETE CASCADE, and dropping the old table with
-- foreign keys on would empty them. Rust checks before committing that no
-- foreign key is broken (SQLite's procedure for changing a table).
--
-- The new kinds need a new CHECK, so `db_properties` is rebuilt: new table,
-- copy (rowid too), drop the old one, rename the new one. The tables that
-- point at it name it, so they point at the new one once renamed.
CREATE TABLE db_properties_v14 (
    id          TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN (
        'text', 'number', 'checkbox', 'date', 'select',
        'multi_select', 'url', 'created', 'edited'
    )),
    position    INTEGER NOT NULL,
    deleted_at  TEXT,
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);

INSERT INTO db_properties_v14 (rowid, id, database_id, name, kind, position, deleted_at,
                               created_at, updated_at)
SELECT rowid, id, database_id, name, kind, position, deleted_at, created_at, updated_at
FROM db_properties;

DROP TABLE db_properties;
ALTER TABLE db_properties_v14 RENAME TO db_properties;
CREATE INDEX db_properties_database_idx ON db_properties(database_id, position);

-- An option belongs to one property: the key the cells of a multi-select
-- point at, so a cell can only hold options of its own property.
CREATE UNIQUE INDEX db_select_options_property_key ON db_select_options(id, property_id);

-- One row per option chosen in a multi-select cell; an empty cell has none.
-- Shown in the order of the property's options. Deleting an option takes it
-- out of every cell.
CREATE TABLE db_multi_values (
    page_id     TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id TEXT NOT NULL REFERENCES db_properties(id) ON DELETE CASCADE,
    option_id   TEXT NOT NULL,
    PRIMARY KEY (page_id, property_id, option_id),
    FOREIGN KEY (option_id, property_id)
        REFERENCES db_select_options(id, property_id) ON DELETE CASCADE
);
CREATE INDEX db_multi_values_property_idx ON db_multi_values(property_id);
CREATE INDEX db_multi_values_option_idx ON db_multi_values(option_id, property_id);
