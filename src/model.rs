use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
    pub kind: PageKind,
}

/// A normal page, or a database whose child pages are its rows (ADR 0006).
/// Stored as text; see [`PageKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum PageKind {
    #[default]
    Page,
    Database,
}

impl PageKind {
    /// Name stored in SQLite (must match the CHECK in migration 0005).
    pub fn as_str(self) -> &'static str {
        match self {
            PageKind::Page => "page",
            PageKind::Database => "database",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [PageKind::Page, PageKind::Database]
            .into_iter()
            .find(|k| k.as_str() == name)
    }
}

/// Block types (ADR 0003). Stored as text; see [`BlockKind::as_str`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum BlockKind {
    Paragraph,
    Heading1,
    Heading2,
    Heading3,
    Todo,
    Bulleted,
    Numbered,
    Quote,
    Code,
}

impl BlockKind {
    pub const ALL: [BlockKind; 9] = [
        BlockKind::Paragraph,
        BlockKind::Heading1,
        BlockKind::Heading2,
        BlockKind::Heading3,
        BlockKind::Todo,
        BlockKind::Bulleted,
        BlockKind::Numbered,
        BlockKind::Quote,
        BlockKind::Code,
    ];

    /// Name stored in SQLite (must match the CHECK in migration 0003).
    pub fn as_str(self) -> &'static str {
        match self {
            BlockKind::Paragraph => "paragraph",
            BlockKind::Heading1 => "heading_1",
            BlockKind::Heading2 => "heading_2",
            BlockKind::Heading3 => "heading_3",
            BlockKind::Todo => "todo",
            BlockKind::Bulleted => "bulleted",
            BlockKind::Numbered => "numbered",
            BlockKind::Quote => "quote",
            BlockKind::Code => "code",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        Self::ALL.into_iter().find(|k| k.as_str() == name)
    }

    /// Name shown in the UI.
    pub fn label(self) -> &'static str {
        match self {
            BlockKind::Paragraph => "Texto",
            BlockKind::Heading1 => "Título 1",
            BlockKind::Heading2 => "Título 2",
            BlockKind::Heading3 => "Título 3",
            BlockKind::Todo => "Tarea",
            BlockKind::Bulleted => "Lista",
            BlockKind::Numbered => "Lista numerada",
            BlockKind::Quote => "Cita",
            BlockKind::Code => "Código",
        }
    }

    pub fn is_heading(self) -> bool {
        matches!(
            self,
            BlockKind::Heading1 | BlockKind::Heading2 | BlockKind::Heading3
        )
    }
}

/// How a database view shows its rows (ADR 0006). Board is H6.5.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ViewKind {
    Table,
    Board,
}

impl ViewKind {
    /// Name stored in SQLite (must match the CHECK in migration 0005).
    pub fn as_str(self) -> &'static str {
        match self {
            ViewKind::Table => "table",
            ViewKind::Board => "board",
        }
    }

    pub fn parse(name: &str) -> Option<Self> {
        [ViewKind::Table, ViewKind::Board]
            .into_iter()
            .find(|k| k.as_str() == name)
    }
}

/// A saved way of looking at a database (its configuration comes in H6.3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DatabaseView {
    pub id: String,
    pub name: String,
    pub kind: ViewKind,
}

/// Deepest indent level a block can have (CHECK in migration 0003).
pub const MAX_INDENT: u8 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub page_id: String,
    pub kind: BlockKind,
    pub content: String,
    pub checked: bool,
    pub position: i64,
    /// Nesting level, 0..=MAX_INDENT (flat list with indent, ADR 0003).
    pub indent: u8,
}

impl Block {
    pub fn is_todo(&self) -> bool {
        self.kind == BlockKind::Todo
    }
}

/// A block to create (import), before it has an id or a page.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NewBlock {
    pub kind: BlockKind,
    pub indent: u8,
    pub checked: bool,
    pub content: String,
}

/// Number shown before each numbered block (`None` for other blocks).
/// One counter per indent level: a non-numbered block at the same level ends
/// the list, and going back to a shallower level restarts deeper lists.
pub fn list_numbers(blocks: &[Block]) -> Vec<Option<usize>> {
    let mut counters = [0usize; MAX_INDENT as usize + 1];
    blocks
        .iter()
        .map(|block| {
            let level = usize::from(block.indent.min(MAX_INDENT));
            for deeper in &mut counters[level + 1..] {
                *deeper = 0;
            }
            if block.kind == BlockKind::Numbered {
                counters[level] += 1;
                Some(counters[level])
            } else {
                counters[level] = 0;
                None
            }
        })
        .collect()
}

/// One visible row of the page tree.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TreeRow {
    pub page: Page,
    pub depth: usize,
    pub has_children: bool,
}

/// Effective parent: a parent that is missing (e.g. trashed) puts the page at the root,
/// so it never disappears from navigation.
fn effective_parent<'a>(page: &'a Page, live: &HashSet<&str>) -> Option<&'a str> {
    page.parent_id.as_deref().filter(|p| live.contains(p))
}

/// Flattens `pages` (already in sibling order) into visible tree rows.
/// Children of collapsed pages are hidden. Never loops, even if the stored
/// data contained a cycle: pages caught in one are shown at the root.
pub fn build_tree(pages: &[Page], expanded: &HashSet<String>) -> Vec<TreeRow> {
    let live: HashSet<&str> = pages.iter().map(|p| p.id.as_str()).collect();
    let mut children: HashMap<Option<&str>, Vec<&Page>> = HashMap::new();
    for page in pages {
        children
            .entry(effective_parent(page, &live))
            .or_default()
            .push(page);
    }

    let mut rows = Vec::with_capacity(pages.len());
    let mut visited: HashSet<&str> = HashSet::new();
    let roots = children.get(&None).cloned().unwrap_or_default();
    let unreached = || pages.iter();
    for root in roots.into_iter().chain(unreached()) {
        if visited.contains(root.id.as_str()) {
            continue;
        }
        // Iterative DFS: (page, depth, show this row).
        let mut stack = vec![(root, 0usize, true)];
        while let Some((page, depth, visible)) = stack.pop() {
            if !visited.insert(page.id.as_str()) {
                continue;
            }
            let kids: Vec<&Page> = children
                .get(&Some(page.id.as_str()))
                .map(|k| {
                    k.iter()
                        .copied()
                        .filter(|c| !visited.contains(c.id.as_str()))
                        .collect()
                })
                .unwrap_or_default();
            if visible {
                rows.push(TreeRow {
                    page: page.clone(),
                    depth,
                    has_children: !kids.is_empty(),
                });
            }
            let show_kids = visible && expanded.contains(&page.id);
            for kid in kids.into_iter().rev() {
                stack.push((kid, depth + 1, show_kids));
            }
        }
    }
    rows
}

/// The pages the sidebar shows: everything except the rows of databases and
/// their subpages (ADR 0006: rows are opened from the database's table, the
/// search or favorites). Removing a whole branch keeps `build_tree` from
/// putting a row's subpage at the root as if it were an orphan. A database
/// itself is always shown, so even a stored cycle never hides one.
pub fn without_database_rows(pages: &[Page]) -> Vec<Page> {
    let mut children: HashMap<&str, Vec<&str>> = HashMap::new();
    for page in pages {
        if let Some(parent) = page.parent_id.as_deref() {
            children.entry(parent).or_default().push(page.id.as_str());
        }
    }
    // Linear walk down from every database; `insert` stops on cycles.
    let mut hidden: HashSet<&str> = HashSet::new();
    let mut stack: Vec<&str> = pages
        .iter()
        .filter(|p| p.kind == PageKind::Database)
        .map(|p| p.id.as_str())
        .collect();
    while let Some(id) = stack.pop() {
        for child in children.get(id).into_iter().flatten() {
            if hidden.insert(child) {
                stack.push(child);
            }
        }
    }
    pages
        .iter()
        .filter(|p| !hidden.contains(p.id.as_str()) || p.kind == PageKind::Database)
        .cloned()
        .collect()
}

/// Ids of all descendants of `id` (not including `id`).
pub fn descendants(pages: &[Page], id: &str) -> HashSet<String> {
    let mut found = HashSet::new();
    let mut frontier = vec![id.to_owned()];
    while let Some(current) = frontier.pop() {
        for page in pages {
            if page.parent_id.as_deref() == Some(current.as_str()) && found.insert(page.id.clone())
            {
                frontier.push(page.id.clone());
            }
        }
    }
    found.remove(id);
    found
}

/// Ids of the ancestors of `id`, nearest first. Stops on missing parents and cycles.
pub fn ancestors(pages: &[Page], id: &str) -> Vec<String> {
    let by_id: HashMap<&str, &Page> = pages.iter().map(|p| (p.id.as_str(), p)).collect();
    let mut chain = Vec::new();
    let mut current = by_id.get(id).and_then(|p| p.parent_id.as_deref());
    while let Some(parent) = current {
        if parent == id || chain.iter().any(|c| c == parent) {
            break;
        }
        let Some(page) = by_id.get(parent) else {
            break;
        };
        chain.push(parent.to_owned());
        current = page.parent_id.as_deref();
    }
    chain
}

/// A page shown in the trash: the page the user trashed, with the subpages
/// that went to the trash together with it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrashItem {
    pub page: Page,
    /// UTC ISO-8601 timestamp shared by the whole trashed branch.
    pub deleted_at: String,
    pub subpages: usize,
}
