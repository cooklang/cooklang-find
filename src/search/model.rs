use std::path::PathBuf;

/// A search result with its relevance score
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub path: PathBuf,
    pub score: f64,
}

impl Eq for SearchResult {}

impl SearchResult {
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path, score: 0.0 }
    }

    pub(crate) fn add_score(&mut self, points: f64) {
        self.score += points;
    }
}
