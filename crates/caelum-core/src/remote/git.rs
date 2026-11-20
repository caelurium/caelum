/// Proof of exactly what was reviewed. A mutation never resolves its target from UI selection.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Revision {
    pub branch: String,
    pub head: String,
    pub index: String,
    pub tree: String,
}

#[derive(Debug, Clone)]
pub enum GitAction {
    Read,
    Commit(Revision, String),
    Push(Revision),
}

#[derive(Debug)]
pub struct Review {
    pub revision: Revision,
    pub status: String,
    pub patch: String,
}

#[derive(Debug)]
pub enum Changes {
    Loading { request: u64, mutation: bool },
    Ready(Review),
    Failed(String),
}
