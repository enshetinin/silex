-- Schema version 15: the calendar view (H13.5), and the list and gallery
-- views of H13.6 in the same CHECK, so that story needs no migration.
-- Applied transactionally by Rust, after a verified copy of the v14 file,
-- with foreign keys off (SQLite's procedure for changing a table). No
-- table points at `db_views`: the last view of each database is kept by
-- id in `settings`, and ids do not change.
--
-- The new kinds need a new CHECK, so `db_views` is rebuilt: new table,
-- copy (rowid too), drop the old one, rename the new one.
CREATE TABLE db_views_v15 (
    id          TEXT PRIMARY KEY,
    database_id TEXT NOT NULL REFERENCES pages(id) ON DELETE CASCADE,
    name        TEXT NOT NULL,
    kind        TEXT NOT NULL CHECK (kind IN ('table', 'board', 'calendar', 'list', 'gallery')),
    position    INTEGER NOT NULL,
    config      TEXT NOT NULL DEFAULT '{}'   -- JSON, validated in Rust (unknown keys ignored)
);

INSERT INTO db_views_v15 (rowid, id, database_id, name, kind, position, config)
SELECT rowid, id, database_id, name, kind, position, config
FROM db_views;

DROP TABLE db_views;
ALTER TABLE db_views_v15 RENAME TO db_views;
