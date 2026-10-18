-- Schema version 5: databases (Hito 6, docs/decisions/0006-databases.md).
-- Additive only; applied transactionally by Rust. A database is a page with
-- kind = 'database'; its rows are its child pages. Property values live in
-- the column of their type; Rust checks that it matches the property.
ALTER TABLE pages ADD COLUMN kind TEXT NOT NULL DEFAULT 'page'
    CHECK (kind IN ('page', 'database'));

CREATE TABLE db_properties (
    id          TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('text', 'number', 'checkbox', 'date', 'select')),
    position    INTEGER NOT NULL,
    deleted_at  TEXT,            -- soft delete: can be restored
    created_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    updated_at  TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now'))
);
CREATE INDEX db_properties_database_idx ON db_properties(database_id, position);

CREATE TABLE db_select_options (
    id          TEXT PRIMARY KEY,
    property_id TEXT NOT NULL REFERENCES db_properties(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    color       TEXT NOT NULL CHECK (color IN ('gray', 'blue', 'green', 'yellow', 'orange', 'red', 'purple', 'pink')),
    position    INTEGER NOT NULL
);

-- One value per row and property, in the column of its type.
CREATE TABLE db_values (
    page_id      TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    property_id  TEXT NOT NULL REFERENCES db_properties(id) ON DELETE CASCADE,
    text_value   TEXT,
    number_value REAL,
    bool_value   INTEGER CHECK (bool_value IN (0, 1)),
    date_value   TEXT CHECK (date_value IS NULL OR date(date_value) IS date_value),
    option_id    TEXT REFERENCES db_select_options(id) ON DELETE SET NULL,
    updated_at   TEXT NOT NULL DEFAULT (strftime('%Y-%m-%dT%H:%M:%fZ', 'now')),
    PRIMARY KEY (page_id, property_id)
);
CREATE INDEX db_values_property_idx ON db_values(property_id);

CREATE TABLE db_views (
    id          TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('table', 'board')),
    position    INTEGER NOT NULL,
    config      TEXT NOT NULL DEFAULT '{}'   -- JSON, validated in Rust (unknown keys ignored)
);
