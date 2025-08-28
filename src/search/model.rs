use camino::Utf8PathBuf;

/// Represents a search result with its relevance score.
///
/// Used internally by the search algorithm to rank results
/// by relevance before returning them to the caller.
#[derive(Debug, Clone, PartialEq)]
pub struct SearchResult {
    pub path: Utf8PathBuf,
    pub score: f64,
}

impl Eq for SearchResult {}

impl SearchResult {
    pub(crate) fn new(path: Utf8PathBuf) -> Self {
        Self { path, score: 0.0 }
    }

    pub(crate) fn add_score(&mut self, points: f64) {
        self.score += points;
    }
}
