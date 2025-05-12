use crate::RecipeEntry;
use std::fs::File;
use std::io::{self, BufRead, BufReader};
use std::path::{Path, PathBuf};
use thiserror::Error;

mod model;

use model::*;

#[derive(Error, Debug)]
pub enum SearchError {
    #[error("Failed to read directory: {0}")]
    GlobError(#[from] glob::GlobError),

    #[error("Failed to create glob pattern: {0}")]
    PatternError(#[from] glob::PatternError),

    #[error("Failed to process recipe: {0}")]
    RecipeEntryError(#[from] crate::RecipeEntryError),

    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
}

/// Search for recipes containing the exact given text
pub fn search(base_dir: &Path, query: &str) -> Result<Vec<RecipeEntry>, SearchError> {
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

/// Search for .cook files in a directory and return scored results
fn search_paths(base_dir: &Path, query: &str) -> Result<Vec<PathBuf>, SearchError> {
    let mut scored_results = vec![];
    let query_lower = query.to_lowercase();
    let terms: Vec<String> = query_lower.split_whitespace().map(String::from).collect();

    let pattern = base_dir.join("**/*.cook");
    let pattern = pattern.to_string_lossy();

    for entry in glob::glob(&pattern)? {
        let path = entry?;
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

    // Sort results by score
    sort_results(&mut scored_results);
    // Return only the paths in sorted order
    Ok(scored_results.into_iter().map(|r| r.path).collect())
}

/// Calculate score for filename matches
fn score_filename_match(path: &Path, query: &str) -> f64 {
    let query = query.to_lowercase();
    path.file_stem()
        .and_then(|name| name.to_str())
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
fn score_content_matches(path: &Path, terms: &[String]) -> io::Result<f64> {
    let matches = count_matches(path, terms)?;
    if matches > 0 {
        // Base score for having any match
        let mut score = 1.0;
        // Additional score for multiple matches (capped)
        score += (0.1 * matches as f64).min(5.0);
        Ok(score)
    } else {
        Ok(0.0)
    }
}

/// Count how many times the terms appear in the file
fn count_matches(path: &Path, terms: &[String]) -> io::Result<usize> {
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
        let a_name = a
            .path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        let b_name = b
            .path
            .file_stem()
            .and_then(|n| n.to_str())
            .unwrap_or("")
            .to_lowercase();

        a_name.cmp(&b_name)
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(format!("{}.cook", name));
        fs::write(&path, content).unwrap();
        path
    }

    fn setup_test_recipes() -> TempDir {
        let temp_dir = TempDir::new().unwrap();

        // Create some test recipes
        create_test_recipe(
            temp_dir.path(),
            "pancakes",
            r#">> servings: 4

            Make delicious pancakes with @maple syrup{}"#,
        );
        create_test_recipe(
            temp_dir.path(),
            "waffles",
            r#">> servings: 2

            Crispy @waffles with @syrup"#,
        );
        create_test_recipe(
            temp_dir.path(),
            "french_toast",
            r#">> servings: 3

            Classic french toast recipe"#,
        );

        // Create nested directories with recipes
        let breakfast_dir = temp_dir.path().join("breakfast");
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
        let results = search(temp_dir.path(), "pancakes").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_search_content_match() {
        let temp_dir = setup_test_recipes();
        let results = search(temp_dir.path(), "syrup").unwrap();

        assert_eq!(results.len(), 2);
        // Both pancakes and waffles should be found as they contain "syrup"
        let names: Vec<_> = results.iter().map(|r| r.name().as_ref().unwrap()).collect();
        assert!(names.iter().any(|n| *n == "pancakes"));
        assert!(names.iter().any(|n| *n == "waffles"));
    }

    #[test]
    fn test_search_case_insensitive() {
        let temp_dir = setup_test_recipes();
        let results = search(temp_dir.path(), "PANCAKES").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_search_multiple_terms() {
        let temp_dir = setup_test_recipes();
        let results = search(temp_dir.path(), "cheese mushroom").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "omelette");
    }

    #[test]
    fn test_search_no_results() {
        let temp_dir = setup_test_recipes();
        let results = search(temp_dir.path(), "nonexistent").unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn test_search_nested_directory() {
        let temp_dir = setup_test_recipes();
        let results = search(temp_dir.path(), "omelette").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "omelette");
    }

    #[test]
    fn test_score_filename_match() {
        let temp_dir = setup_test_recipes();
        let path = temp_dir.path().join("pancakes.cook");

        // Test exact match
        assert!(score_filename_match(&path, "pancakes") > 0.0);
        // Test partial match
        assert!(score_filename_match(&path, "pan") > 0.0);
        // Test no match
        assert_eq!(score_filename_match(&path, "waffle"), 0.0);
        // Test case insensitive
        assert!(score_filename_match(&path, "PANCAKES") > 0.0);
    }

    #[test]
    fn test_score_content_matches() {
        let temp_dir = setup_test_recipes();
        let path = create_test_recipe(
            temp_dir.path(),
            "test",
            r#"word word word
other line
word again"#,
        );

        // Test single term
        let terms = vec![String::from("word")];
        assert!(score_content_matches(&path, &terms).unwrap() > 0.0);

        // Test multiple terms
        let terms = vec![String::from("word"), String::from("line")];
        assert!(score_content_matches(&path, &terms).unwrap() > 0.0);

        // Test no matches
        let terms = vec![String::from("nonexistent")];
        assert_eq!(score_content_matches(&path, &terms).unwrap(), 0.0);
    }

    #[test]
    fn test_search_result_sorting() {
        let mut results = vec![
            SearchResult {
                path: PathBuf::from("b.cook"),
                score: 1.0,
            },
            SearchResult {
                path: PathBuf::from("a.cook"),
                score: 1.0,
            },
            SearchResult {
                path: PathBuf::from("c.cook"),
                score: 2.0,
            },
        ];

        sort_results(&mut results);

        // Should be sorted by score first (highest first), then by name
        assert_eq!(results[0].path, PathBuf::from("c.cook")); // Highest score
        assert_eq!(results[1].path, PathBuf::from("a.cook")); // Same score, alphabetically first
        assert_eq!(results[2].path, PathBuf::from("b.cook")); // Same score, alphabetically second
    }

    #[test]
    fn test_invalid_directory() {
        let result = search(Path::new("/nonexistent/directory"), "query");
        assert!(result.is_ok()); // Search should succeed but return empty results
        assert!(result.unwrap().is_empty());
    }
}
