//! Markdown conversion for pages (milestone 4). Pure functions, no I/O, no egui.
//!
//! Export escapes every character Markdown could interpret and writes the
//! inline formatting (ADR 0015) as Markdown: importing an exported page
//! gives back the exact same text and formatting (see `tests/markdown_*.rs`).
mod export;
mod import;

pub use export::{FileLink, database_to_markdown, page_to_markdown, page_to_markdown_with};
pub use import::{ImportedPage, markdown_to_page};
