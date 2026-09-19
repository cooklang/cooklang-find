//! Recipe searching functionality.
//!
//! This module provides full-text search capabilities for recipe files,
//! supporting both filename and content matching with relevance scoring.

use crate::model::lossy::lines_lossy;
use crate::model::{RecipeEntry, RecipeEntryError};
use camino::{Utf8Path, Utf8PathBuf};
use std::fs::File;
use std::io::{self, BufReader};
use thiserror::Error;

mod filter;
mod model;

pub use filter::{Condition, MetadataFilter, OneOrMany};
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

/// Lists every `.cook`/`.menu` recipe under `base_dir` whose frontmatter
/// satisfies `filter`, without reading past the frontmatter of any file (see
/// [`RecipeEntry::from_path`], which reads only the YAML front matter block).
///
/// Unlike [`search`], a file that can't be turned into a `RecipeEntry` (an
/// I/O error — a permissions problem, a file that disappears mid-walk, and
/// so on) is skipped rather than aborting the whole listing: it can't
/// satisfy any filter, so the metadata-only contract this function makes is
/// "here is everything that does", not "here is everything, or nothing if
/// one file was unreadable". See the crate's `search_with_filter` docs for
/// why this differs from `search`'s behavior, which this function does not
/// change.
///
/// Results are sorted by path for stable output (there is no relevance
/// score to sort by, since there is no query).
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::{filter_by_metadata, MetadataFilter};
/// use camino::Utf8Path;
///
/// let filter = MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "Japanese"}}}"#)?;
/// let recipes = filter_by_metadata(Utf8Path::new("./recipes"), &filter)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn filter_by_metadata(
    base_dir: &Utf8Path,
    filter: &MetadataFilter,
) -> Result<Vec<RecipeEntry>, SearchError> {
    let paths = walk_recipe_paths(base_dir)?;
    let mut recipes = Vec::new();

    for path in paths {
        let Ok(recipe) = RecipeEntry::from_path(path) else {
            // Can't read this file's frontmatter, so it can't satisfy any
            // filter; skip it rather than failing the whole listing.
            continue;
        };
        if filter.matches(&recipe) {
            recipes.push(recipe);
        }
    }

    Ok(recipes)
}

/// Runs [`search`] (same scoring, same relevance order) and keeps only the
/// results that also satisfy `filter`.
///
/// A blank (empty or whitespace-only) `query` is equivalent to
/// [`filter_by_metadata`]: no filename/content scoring is performed, so no
/// recipe body is read unless `filter` itself needs one — which it never
/// does, since `MetadataFilter` only ever looks at frontmatter. A non-blank
/// query does read recipe bodies, exactly as `search` already does, in
/// order to score content matches.
///
/// As with [`filter_by_metadata`], a file that can't be turned into a
/// `RecipeEntry` is skipped rather than aborting the whole search.
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::{search_with_filter, MetadataFilter};
/// use camino::Utf8Path;
///
/// let filter = MetadataFilter::from_json(r#"{"where": {"tags": {"has": "korean"}}}"#)?;
/// let recipes = search_with_filter(Utf8Path::new("./recipes"), "stew", &filter)?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn search_with_filter(
    base_dir: &Utf8Path,
    query: &str,
    filter: &MetadataFilter,
) -> Result<Vec<RecipeEntry>, SearchError> {
    if query.trim().is_empty() {
        return filter_by_metadata(base_dir, filter);
    }

    let paths = search_paths(base_dir, query)?;
    let mut recipes = Vec::new();

    for path in paths {
        let Ok(recipe) = RecipeEntry::from_path(path) else {
            continue;
        };
        if filter.matches(&recipe) {
            recipes.push(recipe);
        }
    }

    Ok(recipes)
}

/// Every `.cook`/`.menu` path under `base_dir`, sorted for stable output.
/// Does no scoring and opens no files — the pure directory walk that backs
/// [`filter_by_metadata`].
fn walk_recipe_paths(base_dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>, SearchError> {
    let mut paths = Vec::new();
    let patterns = [
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
            paths.push(path);
        }
    }

    paths.sort();
    Ok(paths)
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
///
/// Lines are decoded lossily, so a recipe carrying a stray non-UTF-8 byte is
/// still scored on the text around it instead of scoring zero.
fn count_matches(path: &Utf8Path, terms: &[String]) -> io::Result<usize> {
    let file = File::open(path)?;
    let reader = BufReader::new(file);
    let mut total_matches = 0;

    for line in lines_lossy(reader) {
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

    /// A recipe whose bytes are not valid UTF-8 must not break the search.
    ///
    /// Before this, `search` propagated the `InvalidData` error that
    /// `BufRead::lines` raises for such a file, so a single Latin-1 recipe
    /// failed every query that reached it — and the caller only learned that
    /// "stream did not contain valid UTF-8" somewhere under the root
    /// (<https://github.com/cooklang/cookcli/issues/498>).
    #[test]
    fn test_search_tolerates_files_that_are_not_valid_utf8() {
        let temp_dir = setup_test_recipes();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        // Latin-1: the 0xe9 stands for an "é" that never made it to UTF-8.
        fs::write(
            temp_dir_path.join("tuna mornay.cook"),
            b"---\ntitle: Tuna Mornay \xe9\n---\n\nBake @tuna{1%can} with cr\xe8me.\n",
        )
        .unwrap();

        // Matched by file name, which is how the reported failure was reached:
        // the file scores even though its content could not be read.
        let by_name =
            search(&temp_dir_path, "mornay").expect("a bad byte must not fail the search");
        assert_eq!(by_name.len(), 1);
        assert_eq!(by_name[0].name().as_ref().unwrap(), "Tuna Mornay \u{fffd}");

        // And matched by content, so the recipe is still findable by the
        // ingredient the user was actually looking for.
        let by_content =
            search(&temp_dir_path, "tuna").expect("a bad byte must not fail the search");
        assert_eq!(by_content.len(), 1);
        assert_eq!(
            by_content[0].path().unwrap().file_name().unwrap(),
            "tuna mornay.cook"
        );

        // The readable text around the bad byte is still readable.
        assert!(by_content[0]
            .content()
            .unwrap()
            .contains("Bake @tuna{1%can}"));

        // Every other recipe is unaffected.
        assert_eq!(search(&temp_dir_path, "pancakes").unwrap().len(), 1);
    }

    #[test]
    fn test_invalid_directory() {
        let result = search(Utf8Path::new("/nonexistent/directory"), "query");
        assert!(result.is_ok()); // Search should succeed but return empty results
        assert!(result.unwrap().is_empty());
    }

    // ---- filter_by_metadata / search_with_filter ----

    fn create_test_menu(dir: &Utf8Path, name: &str, content: &str) -> Utf8PathBuf {
        let path = dir.join(format!("{name}.menu"));
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn filter_by_metadata_keeps_only_matching_recipes() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&dir, "kimchi_stew", "---\ncuisine: Korean\n---\n\nBody");
        create_test_recipe(&dir, "ramen", "---\ncuisine: Japanese\n---\n\nBody");

        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "Korean"}}}"#).unwrap();
        let results = filter_by_metadata(&dir, &filter).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "kimchi_stew");
    }

    #[test]
    fn filter_by_metadata_includes_menu_files() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_menu(&dir, "this_week", "---\ntheme: Korean\n---\n\n## Monday\n");

        let filter =
            MetadataFilter::from_json(r#"{"where": {"theme": {"equals": "Korean"}}}"#).unwrap();
        let results = filter_by_metadata(&dir, &filter).unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].is_menu());
    }

    #[test]
    fn filter_by_metadata_with_an_empty_filter_matches_everything() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&dir, "a", "Body only");
        create_test_recipe(&dir, "b", "---\ntitle: B\n---\n\nBody");

        let results = filter_by_metadata(&dir, &MetadataFilter::default()).unwrap();
        assert_eq!(results.len(), 2);
    }

    #[test]
    fn filter_by_metadata_sorts_results_by_path() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&dir, "waffles", "Body");
        create_test_recipe(&dir, "apples", "Body");
        create_test_recipe(&dir, "muffins", "Body");

        let results = filter_by_metadata(&dir, &MetadataFilter::default()).unwrap();
        let paths: Vec<String> = results
            .iter()
            .map(|r| r.path().unwrap().to_string())
            .collect();
        let mut sorted = paths.clone();
        sorted.sort();
        assert_eq!(paths, sorted);
    }

    /// A file whose frontmatter is malformed YAML must not abort the whole
    /// listing (unlike `search`, which propagates any single file's read
    /// error — see this module's `search_with_filter` docs). Today,
    /// malformed YAML doesn't even make `RecipeEntry::from_path` return an
    /// error: `extract_and_parse_metadata` falls back to empty metadata (see
    /// `model::metadata::parse_yaml_content`), so the file below is
    /// included, just with no metadata — which fails a filter that requires
    /// a key that can't be inherited, but does not stop the well-formed
    /// recipe next to it from being found. `filter_by_metadata` also skips
    /// (rather than aborts on) any file it can't turn into a `RecipeEntry`
    /// at all, e.g. a genuine I/O error, which is the scenario this
    /// behavior most matters for.
    #[test]
    fn filter_by_metadata_does_not_abort_on_a_file_with_unparsable_frontmatter() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&dir, "broken", "---\ninvalid: yaml: content:\n---\n\nBody");
        create_test_recipe(&dir, "fine", "---\ncuisine: Korean\n---\n\nBody");

        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "Korean"}}}"#).unwrap();
        let results = filter_by_metadata(&dir, &filter).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "fine");

        // And the broken file is simply excluded (empty metadata), not an
        // error that stops the listing.
        let everything = filter_by_metadata(&dir, &MetadataFilter::default()).unwrap();
        assert_eq!(everything.len(), 2);
    }

    /// A genuinely unreadable file (as opposed to one with merely malformed
    /// YAML) is the case `filter_by_metadata`'s skip-on-error behavior is
    /// really for. Gated to Unix because Windows ACLs don't map onto a
    /// simple chmod, and skipped when running as root (root routinely
    /// bypasses the permission bits this test relies on), since neither can
    /// demonstrate the behavior this test is checking.
    #[cfg(unix)]
    #[test]
    fn filter_by_metadata_skips_rather_than_aborts_on_an_unreadable_file() {
        use std::os::unix::fs::PermissionsExt;

        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let unreadable =
            create_test_recipe(&dir, "unreadable", "---\ncuisine: Korean\n---\n\nBody");
        create_test_recipe(&dir, "fine", "---\ncuisine: Korean\n---\n\nBody");

        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&unreadable).is_ok() {
            // Running as a user (e.g. root) that ignores permission bits;
            // this test can't demonstrate anything on this machine.
            fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
            return;
        }

        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "Korean"}}}"#).unwrap();
        let results = filter_by_metadata(&dir, &filter).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "fine");

        fs::set_permissions(&unreadable, fs::Permissions::from_mode(0o644)).unwrap();
    }

    #[test]
    fn search_with_filter_with_a_blank_query_is_filter_by_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&dir, "kimchi_stew", "---\ncuisine: Korean\n---\n\nBody");
        create_test_recipe(&dir, "ramen", "---\ncuisine: Japanese\n---\n\nBody");

        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "Korean"}}}"#).unwrap();

        let via_blank_query = search_with_filter(&dir, "   ", &filter).unwrap();
        let via_filter_only = filter_by_metadata(&dir, &filter).unwrap();

        let names = |entries: &[RecipeEntry]| -> Vec<String> {
            entries.iter().map(|e| e.name().clone().unwrap()).collect()
        };
        assert_eq!(names(&via_blank_query), names(&via_filter_only));
    }

    #[test]
    fn search_with_filter_preserves_searchs_relevance_order() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        // "syrup" scores both recipes equally by content (one occurrence
        // each) and neither by filename, so `search`'s tie-break — sort by
        // file stem — decides the order: "pancakes" before "waffles".
        create_test_recipe(
            &dir,
            "waffles",
            "---\ncuisine: American\n---\n\nCrispy @waffles with @syrup",
        );
        create_test_recipe(
            &dir,
            "pancakes",
            "---\ncuisine: American\n---\n\nServe with @maple syrup{}",
        );

        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "American"}}}"#).unwrap();

        let filtered = search_with_filter(&dir, "syrup", &filter).unwrap();
        let unfiltered = search(&dir, "syrup").unwrap();

        let names = |entries: &[RecipeEntry]| -> Vec<String> {
            entries.iter().map(|e| e.name().clone().unwrap()).collect()
        };
        assert_eq!(names(&filtered), names(&unfiltered));
        assert_eq!(
            names(&filtered),
            vec!["pancakes".to_string(), "waffles".to_string()]
        );
    }

    #[test]
    fn search_with_filter_drops_results_the_filter_rejects() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(
            &dir,
            "pancakes",
            "---\ncuisine: American\n---\n\nServe with syrup",
        );
        create_test_recipe(
            &dir,
            "waffles",
            "---\ncuisine: Belgian\n---\n\nServe with syrup",
        );

        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "American"}}}"#).unwrap();
        let results = search_with_filter(&dir, "syrup", &filter).unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name().as_ref().unwrap(), "pancakes");
    }

    /// The most direct proof that `filter_by_metadata` reads only
    /// frontmatter: the recipe file here is a FIFO whose writer sends a
    /// valid, closed frontmatter block and then blocks forever, sending
    /// neither an end-of-file nor a single further byte. `RecipeEntry`'s
    /// frontmatter reader (`lines_lossy`, see `model::lossy`) is lazy line
    /// by line and stops at the closing `---`; if `filter_by_metadata` ever
    /// started reading a recipe's body, this call would block on the pipe
    /// and the `recv_timeout` below would fail the test instead of the call
    /// returning promptly with the one recipe found.
    #[cfg(unix)]
    #[test]
    fn filter_by_metadata_does_not_read_past_the_frontmatter() {
        use std::io::Write;
        use std::process::Command;
        use std::sync::mpsc;
        use std::time::Duration;

        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let fifo_path = dir.join("blocked.cook");

        let status = Command::new("mkfifo")
            .arg(fifo_path.as_str())
            .status()
            .expect("mkfifo must be available to run this test");
        assert!(status.success(), "mkfifo failed to create the test FIFO");

        let writer_path = fifo_path.clone();
        let _writer = std::thread::spawn(move || {
            let mut f = fs::OpenOptions::new()
                .write(true)
                .open(writer_path)
                .expect("opening the FIFO for writing");
            f.write_all(b"---\ntitle: Blocked Recipe\n---\n\n")
                .expect("writing the frontmatter");
            f.flush().expect("flushing the frontmatter");
            // Deliberately never write more, and never close: a reader that
            // tries to read past the frontmatter blocks here indefinitely.
            // The test process exits (killing this thread) once the
            // assertions below are done, so this never actually waits out
            // the sleep.
            std::thread::sleep(Duration::from_secs(600));
        });

        let (tx, rx) = mpsc::channel();
        std::thread::spawn(move || {
            let result = filter_by_metadata(&dir, &MetadataFilter::default());
            let _ = tx.send(result);
        });

        let result = rx
            .recv_timeout(Duration::from_secs(5))
            .expect("filter_by_metadata blocked, so it read past the frontmatter");
        let entries = result.unwrap();

        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].metadata().title(), Some("Blocked Recipe"));
    }
}
