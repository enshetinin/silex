use std::collections::{HashMap, HashSet};

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Page {
    pub id: String,
    pub parent_id: Option<String>,
    pub title: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub id: String,
    pub page_id: String,
    pub kind: String,
    pub content: String,
    pub checked: bool,
    pub position: i64,
}

impl Block {
    pub fn is_todo(&self) -> bool {
        self.kind == "todo"
    }
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

/// Tree rows for pages whose title contains `query` (case-insensitive), plus
/// their ancestors so each match keeps its context. Everything shown is
/// expanded. An empty query matches nothing.
pub fn filter_tree(pages: &[Page], query: &str) -> Vec<TreeRow> {
    let needle = query.trim().to_lowercase();
    if needle.is_empty() {
        return Vec::new();
    }
    let mut keep: HashSet<String> = HashSet::new();
    for page in pages {
        if page.title.to_lowercase().contains(&needle) {
            keep.insert(page.id.clone());
            keep.extend(ancestors(pages, &page.id));
        }
    }
    let subset: Vec<Page> = pages
        .iter()
        .filter(|p| keep.contains(&p.id))
        .cloned()
        .collect();
    build_tree(&subset, &keep)
}
