-- Schema version 2: favorites. Additive only; applied transactionally by Rust.
-- NULL = not a favorite; the timestamp orders the favorites list (oldest first).
ALTER TABLE pages ADD COLUMN favorite_at TEXT;
CREATE INDEX pages_favorite_idx ON pages(favorite_at) WHERE favorite_at IS NOT NULL;
