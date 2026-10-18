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
