//! Global search (H5.7): FTS5 query building, ranking and snippets, plus
//! [`SearchWorker`], which runs searches off the UI thread.
//!
//! The SQL lives in [`crate::db::Database::search`].
use std::{
    cmp::Ordering,
    path::PathBuf,
    sync::mpsc::{self, Receiver, Sender, TryRecvError},
};

use anyhow::{Context, Result, anyhow};

use crate::db::Database;
use crate::model::Page;

/// Most results a search returns.
pub const MAX_RESULTS: usize = 50;

/// Words of context in a snippet (the `snippet()` token count).
pub(crate) const SNIPPET_TOKENS: i64 = 12;

/// Markers FTS5 puts around each match in `highlight()` and `snippet()`.
/// Control characters, so they are very unlikely in user text. If one does
/// appear there, only the highlighting is off.
pub(crate) const MATCH_START: char = '\u{2}';
pub(crate) const MATCH_END: char = '\u{3}';

/// A run of text, marked when it matched the query.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TextPart {
    pub text: String,
    pub matched: bool,
}

/// The best matching block of a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct BlockMatch {
    pub block_id: String,
    /// A few words around the match, on one line, with "…" where the block
    /// was cut.
    pub snippet: Vec<TextPart>,
    /// Character index of the first match in the block (where to put the cursor).
    pub char_offset: usize,
}

/// One search result: a live page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SearchHit {
    pub page: Page,
    /// The title in parts; nothing is marked when only the content matched.
    pub title: Vec<TextPart>,
    /// Present when the page's content matched.
    pub block: Option<BlockMatch>,
}

impl SearchHit {
    pub fn title_matched(&self) -> bool {
        self.title.iter().any(|part| part.matched)
    }
}

/// FTS5 query for what the user typed: every word must appear, and with
/// `prefix` each word also finds longer words that start with it ("cent"
/// finds "centeno"). Words are kept to letters and digits and quoted, so
/// FTS5 operators in the input (`OR`, `NEAR`, `*`, quotes…) are plain text.
/// `None` when the input has no letters or digits.
pub fn fts_query(input: &str, prefix: bool) -> Option<String> {
    let star = if prefix { "*" } else { "" };
    let words: Vec<String> = input
        .split(|c: char| !c.is_alphanumeric())
        .filter(|word| !word.is_empty())
        .map(|word| format!("\"{word}\"{star}"))
        .collect();
    (!words.is_empty()).then(|| words.join(" AND "))
}

/// Splits `highlight()`/`snippet()` output into parts. Line breaks and
/// tabs become spaces, since results are shown on one line.
pub(crate) fn parse_marked(marked: &str) -> Vec<TextPart> {
    let mut parts: Vec<TextPart> = Vec::new();
    let mut matched = false;
    let mut current = String::new();
    let mut flush = |text: &mut String, matched: bool| {
        if text.is_empty() {
            return;
        }
        match parts.last_mut() {
            Some(last) if last.matched == matched => last.text.push_str(text),
            _ => parts.push(TextPart {
                text: text.clone(),
                matched,
            }),
        }
        text.clear();
    };
    for c in marked.chars() {
        match c {
            MATCH_START | MATCH_END => {
                flush(&mut current, matched);
                matched = c == MATCH_START;
            }
            '\n' | '\r' | '\t' => current.push(' '),
            c => current.push(c),
        }
    }
    flush(&mut current, matched);
    parts
}

/// Character index of the first match in `highlight()` output (0 if none).
/// Nothing is marked before the first marker, so this is also its index in
/// the original text.
pub(crate) fn first_match_offset(marked: &str) -> usize {
    marked.chars().position(|c| c == MATCH_START).unwrap_or(0)
}

/// A page that matched, before ranking. Scores are FTS5 `bm25()`: lower
/// (more negative) is a better match.
#[derive(Debug, Clone)]
pub(crate) struct Candidate {
    pub(crate) page: Page,
    /// Title score and the title marked by `highlight()`.
    pub(crate) title: Option<(f64, String)>,
    /// Best block: id, rowid (to build its snippet) and score.
    pub(crate) block: Option<(String, i64, f64)>,
}

/// Orders results: pages whose title matched come first (best title score
/// first), then pages that only matched in their content (best block first).
/// Ties go to the better content score, then to the title (A–Z) and the id,
/// so the order never depends on the database.
pub(crate) fn rank(candidates: &mut [Candidate]) {
    let content = |c: &Candidate| c.block.as_ref().map(|(_, _, score)| *score);
    candidates.sort_by(|a, b| {
        let tier = |c: &Candidate| c.title.is_none();
        let main = |c: &Candidate| c.title.as_ref().map(|(score, _)| *score).or(content(c));
        tier(a)
            .cmp(&tier(b))
            .then_with(|| compare_scores(main(a), main(b)))
            .then_with(|| compare_scores(content(a), content(b)))
            .then_with(|| {
                a.page
                    .title
                    .to_lowercase()
                    .cmp(&b.page.title.to_lowercase())
            })
            .then_with(|| a.page.id.cmp(&b.page.id))
    });
}

/// Lower is better; a missing score goes last.
fn compare_scores(a: Option<f64>, b: Option<f64>) -> Ordering {
    match (a, b) {
        (Some(a), Some(b)) => a.total_cmp(&b),
        (Some(_), None) => Ordering::Less,
        (None, Some(_)) => Ordering::Greater,
        (None, None) => Ordering::Equal,
    }
}

/// Runs searches on a `silex-search` thread with its own connection, so a
/// slow query (tens of ms with tens of thousands of blocks) never blocks
/// typing. Only the latest request matters: requests queued behind it are
/// skipped and results of older ones are dropped. The thread ends when the
/// worker is dropped.
pub struct SearchWorker {
    requests: Sender<(u64, String)>,
    results: Receiver<(u64, Result<Vec<SearchHit>>)>,
    last_id: u64,
    /// Id of the request whose result is still awaited.
    pending: Option<u64>,
}

impl SearchWorker {
    /// Opens its own connection to `db_path` (WAL: reads while the UI writes).
    /// `on_result` runs after each result is sent (e.g. to repaint the UI).
    pub fn spawn(db_path: PathBuf, on_result: impl Fn() + Send + 'static) -> Result<Self> {
        let (requests, inbox) = mpsc::channel::<(u64, String)>();
        let (outbox, results) = mpsc::channel();
        std::thread::Builder::new()
            .name("silex-search".into())
            .spawn(move || {
                let db = Database::open(&db_path);
                while let Ok(mut request) = inbox.recv() {
                    // Skip to the newest request.
                    while let Ok(newer) = inbox.try_recv() {
                        request = newer;
                    }
                    let result = match &db {
                        Ok(db) => db.search(&request.1),
                        Err(err) => Err(anyhow!("No se puede abrir la base para buscar: {err:#}")),
                    };
                    if outbox.send((request.0, result)).is_err() {
                        break;
                    }
                    on_result();
                }
            })
            .context("No se puede iniciar el hilo de búsqueda")?;
        Ok(Self {
            requests,
            results,
            last_id: 0,
            pending: None,
        })
    }

    /// Searches for `query`; replaces any request still running.
    pub fn request(&mut self, query: String) {
        self.last_id += 1;
        self.pending = Some(self.last_id);
        // If the thread is gone, `poll` reports it.
        let _ = self.requests.send((self.last_id, query));
    }

    /// Forgets the pending request: its result will be dropped.
    pub fn cancel(&mut self) {
        self.pending = None;
    }

    /// A result is still on its way.
    pub fn is_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// The result of the latest request, once, when it arrives.
    pub fn poll(&mut self) -> Option<Result<Vec<SearchHit>>> {
        let wanted = self.pending?;
        loop {
            match self.results.try_recv() {
                Ok((id, result)) if id == wanted => {
                    self.pending = None;
                    return Some(result);
                }
                // An older request: superseded.
                Ok(_) => {}
                Err(TryRecvError::Empty) => return None,
                Err(TryRecvError::Disconnected) => {
                    self.pending = None;
                    return Some(Err(anyhow!("El hilo de búsqueda terminó inesperadamente")));
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn part(text: &str, matched: bool) -> TextPart {
        TextPart {
            text: text.into(),
            matched,
        }
    }

    #[test]
    fn query_quotes_words_and_adds_prefixes() {
        assert_eq!(
            fts_query("pan  de-centeno", true).as_deref(),
            Some("\"pan\"* AND \"de\"* AND \"centeno\"*")
        );
        assert_eq!(fts_query("Ártico", false).as_deref(), Some("\"Ártico\""));
        assert_eq!(
            fts_query("a\"b OR NEAR(", true).as_deref(),
            Some("\"a\"* AND \"b\"* AND \"OR\"* AND \"NEAR\"*")
        );
        for empty in ["", "   ", "\"*()^-", "\u{0}"] {
            assert_eq!(fts_query(empty, true), None, "{empty:?}");
        }
    }

    #[test]
    fn marked_text_becomes_parts_on_one_line() {
        assert_eq!(
            parse_marked("comprar \u{2}pan\u{3} y\nleche"),
            vec![
                part("comprar ", false),
                part("pan", true),
                part(" y leche", false)
            ]
        );
        // Adjacent matches merge; empty runs are dropped.
        assert_eq!(
            parse_marked("\u{2}a\u{3}\u{2}b\u{3}"),
            vec![part("ab", true)]
        );
        assert_eq!(parse_marked(""), Vec::<TextPart>::new());
        // A stray end marker does not break anything.
        assert_eq!(parse_marked("x\u{3}y"), vec![part("xy", false)]);
    }

    #[test]
    fn match_offset_counts_characters() {
        assert_eq!(first_match_offset("añó \u{2}pan\u{3}"), 4);
        assert_eq!(first_match_offset("\u{2}pan\u{3}"), 0);
        assert_eq!(first_match_offset("sin marcas"), 0);
    }

    fn candidate(id: &str, title: Option<f64>, block: Option<f64>) -> Candidate {
        Candidate {
            page: Page {
                id: id.into(),
                parent_id: None,
                title: id.into(),
                kind: Default::default(),
            },
            title: title.map(|s| (s, id.to_owned())),
            block: block.map(|s| (format!("b-{id}"), 0, s)),
        }
    }

    #[test]
    fn titles_rank_before_content_then_by_score() {
        let mut found = vec![
            candidate("contenido-flojo", None, Some(-1.0)),
            candidate("titulo-flojo", Some(-1.0), None),
            candidate("contenido-fuerte", None, Some(-5.0)),
            candidate("titulo-fuerte", Some(-3.0), None),
            candidate("titulo-flojo-con-contenido", Some(-1.0), Some(-2.0)),
        ];
        rank(&mut found);
        let order: Vec<&str> = found.iter().map(|c| c.page.id.as_str()).collect();
        assert_eq!(
            order,
            [
                "titulo-fuerte",
                "titulo-flojo-con-contenido",
                "titulo-flojo",
                "contenido-fuerte",
                "contenido-flojo"
            ]
        );
    }

    #[test]
    fn equal_scores_fall_back_to_title_and_id() {
        let mut found = vec![
            candidate("b", None, Some(-1.0)),
            candidate("A", None, Some(-1.0)),
            candidate("c", None, Some(-1.0)),
        ];
        rank(&mut found);
        let order: Vec<&str> = found.iter().map(|c| c.page.id.as_str()).collect();
        assert_eq!(order, ["A", "b", "c"]);
    }
}
