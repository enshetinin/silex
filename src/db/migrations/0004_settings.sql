-- Schema version 4: user preferences (theme, interface size). Additive only;
-- applied transactionally by Rust. Key/value text so a new preference needs
-- no migration; Rust validates every value when reading.
CREATE TABLE settings (
    key   TEXT PRIMARY KEY NOT NULL,
    value TEXT NOT NULL
);
