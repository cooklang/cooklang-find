//! Recipe searching functionality.
//!
//! This module provides full-text search capabilities for recipe files,
//! supporting both filename and content matching with relevance scoring.

use crate::model::{RecipeEntry, RecipeEntryError};
use camino::{Utf8Path, Utf8PathBuf};
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use thiserror::Error;

mod model;

pub use model::SearchResult;

/// Errors that can occur during recipe searching.
#[derive(Error, Debug)]
pub enum SearchError {
    #[error("Failed to read directory: {0}")]
    GlobError(#[from] glob::GlobError),

    #[error("Failed to create glob pattern: {0}")]
    PatternError(#[from] glob::PatternError),

    #[error("Failed to process recipe: {0}")]
    RecipeEntryError(#[from] RecipeEntryError),

    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
}

/// Searches for recipes in a directory tree that match a query string.
///
/// This function performs a comprehensive search through all .cook and .menu files
/// in the specified directory and its subdirectories. The search algorithm:
///
/// 1. Searches for exact and partial filename matches (highest priority)
/// 2. Searches for query terms within file contents
/// 3. Scores and ranks results by relevance
///
/// # Arguments
///
/// * `base_dir` - The root directory to search in
/// * `query` - The search query (can contain multiple terms separated by spaces)
///
/// # Returns
///
/// Returns a vector of `RecipeEntry` objects sorted by relevance score,
/// with the most relevant recipes first.
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::search;
/// use camino::Utf8Path;
///
/// // Search for recipes containing "chocolate"
/// let results = search(Utf8Path::new("./recipes"), "chocolate")?;
///
/// // Search with multiple terms
/// let results = search(Utf8Path::new("./recipes"), "chocolate cake")?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn search(base_dir: &Utf8Path, query: &str) -> Result<Vec<RecipeEntry>, SearchError> {
    let paths = search_paths(base_dir, query)?;
    let mut recipes = Vec::new();

    for path in paths {
        match RecipeEntry::from_path(path) {
            Ok(recipe) => recipes.push(recipe),
            Err(e) => return Err(SearchError::RecipeEntryError(e)),
        }
    }

    Ok(recipes)
}

/// Search for .cook and .menu files in a directory and return scored results
fn search_paths(base_dir: &Utf8Path, query: &str) -> Result<Vec<Utf8PathBuf>, SearchError> {
    let mut scored_results = vec![];
    let query_lower = query.to_lowercase();
    let terms: Vec<String> = query_lower.split_whitespace().map(String::from).collect();

    // Search for both .cook and .menu files
    let patterns = vec![
        base_dir.join("**/*.cook").to_string(),
        base_dir.join("**/*.menu").to_string(),
    ];

    for pattern in patterns {
        for entry in glob::glob(&pattern)? {
            let path = entry?;
            let path = Utf8PathBuf::from_path_buf(path).map_err(|_| {
                SearchError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Path contains invalid UTF-8",
                ))
            })?;
            let mut result = SearchResult::new(path);

            // Score based on filename match (using full query)
            let filename_score = score_filename_match(&result.path, &query_lower);
            result.add_score(filename_score);

            // Score based on content matches (using individual terms)
            if let Ok(content_score) = score_content_matches(&result.path, &terms) {
                result.add_score(content_score);
            }

            // Include result if it has any score
            if result.score > 0.0 {
                scored_results.push(result);
            }
        }
    }

    // Sort results by score
    sort_results(&mut scored_results);
    // Return only the paths in sorted order
    Ok(scored_results.into_iter().map(|r| r.path).collect())
}

/// Calculate score for filename matches
fn score_filename_match(path: &Utf8Path, query: &str) -> f64 {
    let query = query.to_lowercase();
    path.file_stem()
        .map(|name| {
            let name = name.to_lowercase();
            if name == query {
                20.0 // Highest score for exact match
            } else if name.contains(&query) {
                10.0 // High score for partial match
            } else {
                0.0
            }
        })
        .unwrap_or(0.0)
}

/// Calculate score for content matches
fn score_content_matches(path: &Utf8Path, terms: &[String]) -> io::Result<f64> {
    let matches = count_matches(path, terms)?;
    if matches > 0 {
        // Base score for having any match
        let mut score = 1.0;
        // Additional score for multiple matches (capped)
        score += f64::min(0.1 * matches as f64, 5.0);
        Ok(score)
    } else {
        Ok(0.0)
    }
}

/// Count how many times the terms appear in the file
fn count_matches(path: &Utf8Path, terms: &[String]) -> io::Result<usize> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut total_matches = 0;

    for line in reader.lines() {
        let line = line?.to_lowercase();
        total_matches += terms
            .iter()
            .map(|term| line.matches(term).count())
            .sum::<usize>();
    }

    Ok(total_matches)
}

/// Sort search results by score in descending order
fn sort_results(results: &mut [SearchResult]) {
    results.sort_unstable_by(|a, b| {
        // First sort by score (highest first)
        let score_cmp = b
            .score
            .partial_cmp(&a.score)
            .unwrap_or(std::cmp::Ordering::Equal);

        if score_cmp != std::cmp::Ordering::Equal {
            return score_cmp;
        }

        // If scores are equal, sort by filename
        let a_name = a.path.file_stem().unwrap_or("").to_lowercase();
        let b_name = b.path.file_stem().unwrap_or("").to_lowercase();

        a_name.cmp(&b_name)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Utf8Path, name: &str, content: &str) -> Utf8PathBuf {
        let path = dir.join(format!("{name}.cook"));
        fs::write(&path, content).unwrap();
        path
    }

    fn setup_test_recipes() -> TempDir {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // Create some test recipes
        create_test_recipe(
            &temp_dir_path,
            "pancakes",
            r#">> servings: 4

            Make delicious pancakes with @maple syrup{}"#,
        );
        create_test_recipe(
            &temp_dir_path,
            "waffles",
            r#">> servings: 2

            Crispy @waffles with @syrup"#,
        );
        create_test_recipe(
            &temp_dir_path,
            "french_toast",
            r#">> servings: 3

            Classic french toast recipe"#,
        );

        // Create nested directories with recipes
        let breakfast_dir = temp_dir_path.join("breakfast");
        fs::create_dir_all(&breakfast_dir).unwrap();
        create_test_recipe(
            &breakfast_dir,
            "omelette",
            r#">> servings: 1

            @Cheese and @mushroom omelette"#,
        );

        temp_dir
    }

    #[test]
    fn test_search_exact_match() {
        let temp_dir = setup_test_recipes();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let results = search(&temp_dir_path, "pancakes").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_search_partial_match() {
        let temp_dir = setup_test_recipes();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let results = search(&temp_dir_path, "pancake").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_search_content_match() {
        let temp_dir = setup_test_recipes();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let results = search(&temp_dir_path, "syrup").unwrap();

        assert_eq!(results.len(), 2);
        let names: Vec<String> = results
            .iter()
            .map(|r| r.name().as_ref().unwrap().clone())
            .collect();
        assert!(names.contains(&"pancakes".to_string()));
        assert!(names.contains(&"waffles".to_string()));
    }

    #[test]
    fn test_search_no_matches() {
        let temp_dir = setup_test_recipes();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let results = search(&temp_dir_path, "nonexistent").unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn test_search_result_sorting() {
        let mut results = vec![
            SearchResult {
                path: Utf8PathBuf::from("b.cook"),
                score: 1.0,
            },
            SearchResult {
                path: Utf8PathBuf::from("a.cook"),
                score: 1.0,
            },
            SearchResult {
                path: Utf8PathBuf::from("c.cook"),
                score: 2.0,
            },
        ];

        sort_results(&mut results);

        // Should be sorted by score first (highest first), then by name
        assert_eq!(results[0].path, Utf8PathBuf::from("c.cook")); // Highest score
        assert_eq!(results[1].path, Utf8PathBuf::from("a.cook")); // Same score, alphabetically first
        assert_eq!(results[2].path, Utf8PathBuf::from("b.cook")); // Same score, alphabetically second
    }

    #[test]
    fn test_invalid_directory() {
        let result = search(Utf8Path::new("/nonexistent/directory"), "query");
        assert!(result.is_ok()); // Search should succeed but return empty results
        assert!(result.unwrap().is_empty());
    }
}
