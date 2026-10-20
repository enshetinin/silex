-- Schema version 9: page templates (docs/decisions/0010-templates.md).
-- Applied transactionally by Rust, after a verified copy of the v8 file.
--
-- A template is a page with `template_at` set (when it became one). It is
-- edited like any page but is left out of the page tree, favorites, search
-- and backlinks; «Nueva página desde plantilla» copies it. Rust keeps every
-- template a top-level page of kind 'page'.
--
-- Only a new column: nothing is rebuilt. Existing pages are not templates.
ALTER TABLE pages ADD COLUMN template_at TEXT;

CREATE INDEX pages_template_idx ON pages(template_at) WHERE template_at IS NOT NULL;
