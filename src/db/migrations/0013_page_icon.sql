-- Schema version 13: page icons (H12.8).
-- Applied transactionally by Rust, after a verified copy of the v12 file.
--
-- An emoji shown before a page's title. NULL means no icon (the page or
-- table icon is shown). Rust only stores one emoji (`emoji::is_icon`);
-- the CHECK keeps any value short. Only a new column: nothing is rebuilt.
ALTER TABLE pages ADD COLUMN icon TEXT CHECK (icon IS NULL OR length(icon) BETWEEN 1 AND 16);
