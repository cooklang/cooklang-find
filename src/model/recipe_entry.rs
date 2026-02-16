use super::metadata::{extract_and_parse_metadata, Metadata};
use camino::{Utf8Path, Utf8PathBuf};
use glob::glob;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;
use std::sync::OnceLock;
use thiserror::Error;

/// Represents the complete collection of step images for a recipe.
///
/// Images are discovered based on the Cooklang naming convention:
/// - `RecipeName.N.ext`: Stored at \[0\]\[N-1\] (section 0 = linear/no section)
/// - `RecipeName.S.N.ext`: Stored at \[S-1\]\[N-1\] (section S, step N)
///
/// File numbering is one-indexed (Recipe.1.jpg = first step)
/// Internal HashMap keys are zero-indexed
/// Section 0 is reserved for linear recipes without sections.
///
/// Supported extensions: jpg, jpeg, png, webp
#[derive(Debug, Clone, Serialize, Default)]
pub struct StepImageCollection {
    /// Two-dimensional map: section_index -> step_index -> image_path
    /// - Section 0: steps for linear recipes (Recipe.N.ext stored at \[0\]\[N-1\])
    /// - Section 1+: steps within sections (Recipe.S.N.ext stored at \[S-1\]\[N-1\])
    ///
    /// HashMap keys are zero-indexed
    pub images: HashMap<usize, HashMap<usize, String>>,
}

impl StepImageCollection {
    /// Returns true if there are any images in the collection
    pub fn is_empty(&self) -> bool {
        self.images.is_empty()
    }

    /// Returns total count of all images across all sections
    pub fn count(&self) -> usize {
        self.images.values().map(|steps| steps.len()).sum()
    }

    /// Gets an image for a specific section and step.
    ///
    /// # Arguments
    /// * `section` - Section number (0 for linear recipes, 1+ for sectioned recipes, one-indexed for sections)
    /// * `step` - One-indexed step number (1 = first step)
    ///
    /// # Returns
    /// Image path if found, None otherwise
    ///
    /// # Examples
    /// ```
    /// # use cooklang_find::StepImageCollection;
    /// # let images = StepImageCollection::default();
    /// // Get step 3 in linear recipe (Recipe.3.jpg stored at [0][2])
    /// let img = images.get(0, 3);
    ///
    /// // Get section 2, step 4 (Recipe.2.4.jpg stored at [1][3])
    /// let img = images.get(2, 4);
    /// ```
    pub fn get(&self, section: usize, step: usize) -> Option<&String> {
        if step == 0 {
            return None; // Steps are one-indexed
        }
        // For section 0 (linear recipes), use section index 0
        // For section 1+, convert to zero-indexed (section - 1)
        let section_idx = if section == 0 { 0 } else { section - 1 };
        self.images.get(&section_idx)?.get(&(step - 1))
    }
}

/// Represents the source of a recipe.
///
/// A recipe can come from either:
/// - A file path on the filesystem
/// - Direct content (e.g., from stdin or programmatically created)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "source_type")]
pub enum RecipeSource {
    Path {
        path: Utf8PathBuf,
    },
    Content {
        content: String,
        name: Option<String>,
    },
}

/// Represents a single recipe or menu entry.
///
/// This structure encapsulates all information about a recipe including:
/// - Its source (file path or content)
/// - Metadata extracted from YAML frontmatter
/// - Cached computed values like name and title image
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::RecipeEntry;
/// use camino::Utf8PathBuf;
///
/// // Load a recipe from a file
/// let recipe = RecipeEntry::from_path(Utf8PathBuf::from("recipes/pancakes.cook"))?;
///
/// // Access recipe information
/// let name = recipe.name();
/// let metadata = recipe.metadata();
/// let content = recipe.content()?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
#[derive(Debug, Serialize, Deserialize)]
pub struct RecipeEntry {
    /// Source of the recipe (path or content)
    source: RecipeSource,
    /// Cached metadata
    metadata: Metadata,

    /// Cached name of the recipe (from file stem, title, or provided name)
    #[serde(skip)]
    name: OnceLock<Option<String>>,
    /// Optional path or URL to the title image
    #[serde(skip)]
    title_image: OnceLock<Option<String>>,
    /// Cached step and section images
    #[serde(skip)]
    step_images: OnceLock<StepImageCollection>,
    /// Whether this is a menu file (*.menu) rather than a regular recipe
    #[serde(skip)]
    is_menu: OnceLock<bool>,
}

impl Clone for RecipeEntry {
    fn clone(&self) -> Self {
        RecipeEntry {
            source: self.source.clone(),
            metadata: self.metadata.clone(),
            // Reset cached fields - they will be recomputed on demand
            name: OnceLock::new(),
            title_image: OnceLock::new(),
            step_images: OnceLock::new(),
            is_menu: OnceLock::new(),
        }
    }
}

impl RecipeEntry {
    /// Creates a new `RecipeEntry` from a file path.
    ///
    /// Reads the recipe file, extracts metadata from YAML frontmatter,
    /// and creates a fully initialized recipe entry.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the recipe file (.cook or .menu)
    ///
    /// # Errors
    ///
    /// Returns `RecipeEntryError` if:
    /// - The file cannot be read
    /// - The metadata cannot be parsed
    pub fn from_path(path: Utf8PathBuf) -> Result<Self, RecipeEntryError> {
        let file = File::open(&path).map_err(RecipeEntryError::IoError)?;
        let reader = BufReader::new(file);

        let metadata = extract_and_parse_metadata(
            reader.lines().map(|r| r.map_err(RecipeEntryError::IoError)),
        )?;

        Ok(RecipeEntry {
            source: RecipeSource::Path { path },
            metadata,
            name: OnceLock::new(),
            title_image: OnceLock::new(),
            step_images: OnceLock::new(),
            is_menu: OnceLock::new(),
        })
    }

    /// Creates a new `RecipeEntry` from string content.
    ///
    /// This method is useful for creating recipes from sources other than files,
    /// such as stdin, network responses, or programmatically generated content.
    ///
    /// # Arguments
    ///
    /// * `content` - The full recipe content including any YAML frontmatter
    /// * `name` - Optional name for the recipe (used if no title in metadata)
    ///
    /// # Errors
    ///
    /// Returns `RecipeEntryError` if the metadata cannot be parsed.
    pub fn from_content(content: String, name: Option<String>) -> Result<Self, RecipeEntryError> {
        let metadata = extract_and_parse_metadata(
            content
                .lines()
                .map(|line| Ok::<_, RecipeEntryError>(line.to_string())),
        )?;

        Ok(RecipeEntry {
            source: RecipeSource::Content { content, name },
            metadata,
            name: OnceLock::new(),
            title_image: OnceLock::new(),
            step_images: OnceLock::new(),
            is_menu: OnceLock::new(),
        })
    }

    /// Returns the name of the recipe.
    ///
    /// The name is determined in the following priority order:
    /// 1. Title from metadata (if present)
    /// 2. File stem (for path-based recipes)
    /// 3. Provided name (for content-based recipes)
    ///
    /// The result is cached after the first call.
    pub fn name(&self) -> &Option<String> {
        self.name.get_or_init(|| {
            if let Some(title) = self.metadata.title() {
                Some(title.to_string())
            } else {
                match &self.source {
                    RecipeSource::Path { path } => Some(path.file_stem()?.to_string()),
                    RecipeSource::Content { name, .. } => name.clone(),
                }
            }
        })
    }

    /// Returns the URL or path to the recipe's title image.
    ///
    /// The image is determined in the following priority order:
    /// 1. Image URL from metadata (image, images, picture, or pictures fields)
    /// 2. Image file with same stem as recipe (for path-based recipes)
    ///
    /// Supported image extensions: jpg, jpeg, png, webp
    ///
    /// The result is cached after the first call.
    pub fn title_image(&self) -> &Option<String> {
        self.title_image.get_or_init(|| {
            // First check metadata for image URLs
            if let Some(url) = self.metadata.image_url() {
                return Some(url);
            }

            // For path-based recipes, check for file-based images
            match &self.source {
                RecipeSource::Path { path } => find_title_image(path).map(|p| p.to_string()),
                RecipeSource::Content { .. } => None,
            }
        })
    }

    /// Returns the full content of the recipe.
    ///
    /// For path-based recipes, this reads the file from disk.
    /// For content-based recipes, this returns the stored content.
    ///
    /// # Errors
    ///
    /// Returns `RecipeEntryError::IoError` if the file cannot be read
    /// (only applicable for path-based recipes).
    pub fn content(&self) -> Result<String, RecipeEntryError> {
        match &self.source {
            RecipeSource::Path { path } => {
                std::fs::read_to_string(path).map_err(RecipeEntryError::IoError)
            }
            RecipeSource::Content { content, .. } => Ok(content.clone()),
        }
    }

    /// Returns a reference to the recipe's metadata.
    ///
    /// The metadata contains all fields from the YAML frontmatter,
    /// providing access to both standard fields (title, servings, tags)
    /// and any custom fields defined in the recipe.
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Returns the file path if this recipe is backed by a file.
    ///
    /// Returns `None` for recipes created from content.
    pub fn path(&self) -> Option<&Utf8PathBuf> {
        match &self.source {
            RecipeSource::Path { path } => Some(path),
            RecipeSource::Content { .. } => None,
        }
    }

    /// Returns the file name if this recipe is backed by a file.
    ///
    /// Returns `None` for recipes created from content.
    pub fn file_name(&self) -> Option<String> {
        match &self.source {
            RecipeSource::Path { path } => Some(path.file_name()?.to_string()),
            RecipeSource::Content { .. } => None,
        }
    }

    /// Returns the recipe's tags from metadata.
    ///
    /// Tags can be defined in metadata as:
    /// - A comma-separated string: `tags: "breakfast, easy, vegetarian"`
    /// - An array: `tags: [breakfast, easy, vegetarian]`
    ///
    /// Returns an empty vector if no tags are defined.
    pub fn tags(&self) -> Vec<String> {
        self.metadata.tags()
    }

    /// Checks if this entry represents a menu file.
    ///
    /// Returns `true` if the file has a .menu extension,
    /// `false` otherwise (including content-based recipes).
    pub fn is_menu(&self) -> bool {
        *self.is_menu.get_or_init(|| match &self.source {
            RecipeSource::Path { path } => path.extension() == Some("menu"),
            RecipeSource::Content { .. } => false,
        })
    }

    /// Returns all step and section images for the recipe.
    ///
    /// Images follow the Cooklang naming convention (one-indexed):
    /// - `RecipeName.N.ext`: Step N in linear recipe (stored at section 0)
    /// - `RecipeName.S.N.ext`: Section S, step N within section
    ///
    /// All step and section numbers are one-indexed (first step/section is 1).
    ///
    /// Supported extensions: jpg, jpeg, png, webp (in priority order)
    ///
    /// The result is cached after the first call.
    ///
    /// # Returns
    ///
    /// Reference to StepImageCollection containing all discovered images.
    /// For content-based recipes, returns an empty collection.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use cooklang_find::RecipeEntry;
    /// use camino::Utf8PathBuf;
    ///
    /// let recipe = RecipeEntry::from_path(Utf8PathBuf::from("recipes/pasta.cook"))?;
    /// let images = recipe.step_images();
    ///
    /// // Access linear step image (Pasta.3.jpg)
    /// if let Some(img) = images.get(0, 3) {
    ///     println!("Step 3 image: {}", img);
    /// }
    ///
    /// // Access section-step image (Pasta.2.4.jpg)
    /// if let Some(img) = images.get(2, 4) {
    ///     println!("Section 2, Step 4 image: {}", img);
    /// }
    ///
    /// // Direct HashMap access for iteration
    /// if let Some(section_steps) = images.images.get(&1) {
    ///     for (step_idx, img_path) in section_steps {
    ///         println!("Section 2, Step {}: {}", step_idx + 1, img_path);
    ///     }
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn step_images(&self) -> &StepImageCollection {
        self.step_images.get_or_init(|| match &self.source {
            RecipeSource::Path { path } => find_step_images(path),
            RecipeSource::Content { .. } => StepImageCollection::default(),
        })
    }

    /// Returns all file paths related to this recipe.
    ///
    /// Includes:
    /// - Title image (if any)
    /// - Step/section images
    /// - Referenced recipe .cook files (detected via `@./path` or `@../path` syntax)
    /// - Recursively: related files of referenced recipes
    ///
    /// Returns an empty Vec for content-based recipes.
    /// Missing referenced files are silently skipped.
    /// Cycles are detected and broken automatically.
    pub fn related_files(&self) -> Vec<Utf8PathBuf> {
        let path = match &self.source {
            RecipeSource::Path { path } => path,
            RecipeSource::Content { .. } => return Vec::new(),
        };
        let mut visited = std::collections::HashSet::new();
        let mut result = Vec::new();
        collect_related_files(path, &mut visited, &mut result);
        result
    }
}

/// Errors that can occur when working with recipe entries.
#[derive(Error, Debug)]
pub enum RecipeEntryError {
    #[error("Failed to read recipe file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to get file stem from path: {0}")]
    InvalidPath(Utf8PathBuf),

    #[error("Failed to parse recipe: {0}")]
    ParseError(String),

    #[error("Failed to parse recipe metadata: {0}")]
    MetadataError(String),
}

fn find_title_image(path: &Utf8Path) -> Option<Utf8PathBuf> {
    // Look for an image with the same stem
    let possible_image_extensions = ["jpg", "jpeg", "png", "webp"];
    possible_image_extensions.iter().find_map(|ext| {
        let image_path = path.with_extension(ext);
        if image_path.exists() {
            Some(image_path)
        } else {
            None
        }
    })
}

/// Discovers all step and section images for a recipe file.
///
/// Uses glob patterns to find images matching these patterns:
/// - `Recipe.N.ext` (where N is 1+, one-indexed) → stored at [0][N-1]
/// - `Recipe.S.N.ext` (where S and N are 1+, one-indexed) → stored at [S-1][N-1]
///
/// Images are discovered purely by filename pattern. No recipe parsing required.
///
/// # Arguments
///
/// * `path` - Path to the recipe file
///
/// # Returns
///
/// StepImageCollection containing all discovered images
fn find_step_images(path: &Utf8Path) -> StepImageCollection {
    let mut collection = StepImageCollection::default();
    let stem = match path.file_stem() {
        Some(s) => s,
        None => return collection,
    };
    let dir = path.parent().unwrap_or(path);
    let extensions = ["jpg", "jpeg", "png", "webp"];

    // Build glob pattern for images: Recipe.*.ext and Recipe.*.*.ext
    // Pattern matches: Recipe.1.jpg, Recipe.2.4.png, etc.
    for ext in &extensions {
        let pattern = dir.join(format!("{}.*.{}", stem, ext));
        let pattern_str = pattern.as_str();

        if let Ok(entries) = glob(pattern_str) {
            for entry in entries.flatten() {
                if let Some(numbers) = parse_image_numbers(&entry, stem, ext) {
                    let entry_str = entry.to_string_lossy().to_string();

                    match numbers.len() {
                        // Single number: Recipe.N.ext
                        1 => {
                            let step_num = numbers[0]; // One-indexed from filename
                                                       // Store in section 0 for linear recipes
                                                       // Recipe.1.ext -> [0][0], Recipe.3.ext -> [0][2]
                            collection
                                .images
                                .entry(0)
                                .or_insert_with(HashMap::new)
                                .entry(step_num - 1) // Convert to zero-indexed
                                .or_insert(entry_str);
                        }
                        // Two numbers: Recipe.S.N.ext
                        2 => {
                            let (section_num, step_num) = (numbers[0], numbers[1]); // One-indexed
                                                                                    // Recipe.2.4.ext -> [1][3]
                            collection
                                .images
                                .entry(section_num - 1) // Convert to zero-indexed
                                .or_insert_with(HashMap::new)
                                .entry(step_num - 1) // Convert to zero-indexed
                                .or_insert(entry_str);
                        }
                        _ => {} // Ignore invalid patterns
                    }
                }
            }
        }
    }

    collection
}

/// Parses step/section numbers from an image filename.
///
/// Examples:
/// - "Recipe.3.jpg" -> Some(vec![3])
/// - "Recipe.2.4.jpg" -> Some(vec![2, 4])
/// - "Recipe.invalid.jpg" -> None
///
/// # Arguments
///
/// * `path` - Path to the image file
/// * `stem` - Recipe file stem (e.g., "Recipe")
/// * `ext` - Image extension (e.g., "jpg")
///
/// # Returns
///
/// Vector of one-indexed numbers if valid, None otherwise
fn parse_image_numbers(path: &Path, stem: &str, ext: &str) -> Option<Vec<usize>> {
    let filename = path.file_name()?.to_str()?;

    // Remove the stem and extension to get just the number part(s)
    // Example: "Recipe.2.4.jpg" -> ".2.4."
    let without_stem = filename.strip_prefix(stem)?;
    let without_ext = without_stem.strip_suffix(&format!(".{}", ext))?;

    // Split by dots and parse numbers
    // Example: ".2.4." -> ["", "2", "4", ""]
    let numbers: Vec<usize> = without_ext
        .split('.')
        .filter(|s| !s.is_empty())
        .filter_map(|s| s.parse::<usize>().ok())
        .collect();

    // Only accept 1 or 2 numbers, and they must be >= 1 (one-indexed)
    if !numbers.is_empty() && numbers.len() <= 2 && numbers.iter().all(|&n| n >= 1) {
        Some(numbers)
    } else {
        None
    }
}

/// Extracts recipe references from Cooklang content.
///
/// Looks for ingredient references that are relative file paths,
/// matching patterns like `@./path/to/Recipe` or `@../path/to/Recipe`
/// with optional quantity `{...}`.
///
/// Returns deduplicated list of referenced paths (without extension).
fn extract_recipe_references(content: &str) -> Vec<String> {
    let re = Regex::new(r"@(\.\.?/[^\s\{]+)").unwrap();
    let mut seen = std::collections::HashSet::new();
    let mut refs = Vec::new();
    for cap in re.captures_iter(content) {
        let path = cap[1].to_string();
        if seen.insert(path.clone()) {
            refs.push(path);
        }
    }
    refs
}

/// Recursively collects all files related to a recipe.
///
/// Adds image paths and referenced recipe paths to `result`.
/// Uses `visited` to prevent cycles and deduplication.
fn collect_related_files(
    recipe_path: &Utf8Path,
    visited: &mut std::collections::HashSet<Utf8PathBuf>,
    result: &mut Vec<Utf8PathBuf>,
) {
    // Mark as visited to prevent cycles
    if !visited.insert(recipe_path.to_path_buf()) {
        return;
    }

    // Collect title image
    if let Some(image_path) = find_title_image(recipe_path) {
        result.push(image_path);
    }

    // Collect step images
    let step_images = find_step_images(recipe_path);
    for steps in step_images.images.values() {
        for image_path in steps.values() {
            result.push(Utf8PathBuf::from(image_path));
        }
    }

    // Read content and extract recipe references
    let content = match std::fs::read_to_string(recipe_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let dir = recipe_path.parent().unwrap_or(recipe_path);
    for ref_path_str in extract_recipe_references(&content) {
        // Resolve relative to recipe's directory
        let ref_path = dir.join(&ref_path_str);

        // Try with .cook extension if no extension present
        let candidates = if ref_path.extension().is_some() {
            vec![ref_path]
        } else {
            vec![ref_path.with_extension("cook")]
        };

        for candidate in candidates {
            if candidate.exists() && !visited.contains(&candidate) {
                result.push(candidate.clone());
                collect_related_files(&candidate, visited, result);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Utf8Path, name: &str, content: &str) -> Utf8PathBuf {
        let recipe_path = dir.join(format!("{name}.cook"));
        let mut file = File::create(&recipe_path).unwrap();
        write!(file, "{content}").unwrap();
        recipe_path
    }

    fn create_test_image(dir: &Utf8Path, name: &str, ext: &str) -> Utf8PathBuf {
        let image_path = dir.join(format!("{name}.{ext}"));
        File::create(&image_path).unwrap();
        image_path
    }

    #[test]
    fn test_recipe_creation() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "test_recipe",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );

        let recipe = RecipeEntry::from_path(recipe_path.clone()).unwrap();
        assert_eq!(recipe.name().as_ref().unwrap(), "test_recipe");
        assert_eq!(recipe.path(), Some(&recipe_path));
        assert_eq!(recipe.file_name().as_ref().unwrap(), "test_recipe.cook");
        assert!(recipe.title_image().is_none());
    }

    #[test]
    fn test_recipe_name_from_title() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "test_recipe",
            indoc! {r#"
                ---
                title: My Special Recipe
                servings: 4
                ---

                Test recipe content"#},
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        assert_eq!(recipe.name().as_ref().unwrap(), "My Special Recipe");
    }

    #[test]
    fn test_recipe_with_title_image() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "test_recipe",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );
        let image_path = create_test_image(&temp_dir_path, "test_recipe", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        assert_eq!(
            recipe.title_image().as_ref().unwrap(),
            &image_path.to_string()
        );
    }

    #[test]
    fn test_recipe_content() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let content = indoc! {r#"
            ---
            servings: 4
            ---

            Test recipe content"#};
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", content);

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        assert_eq!(recipe.content().unwrap(), content);
    }

    #[test]
    fn test_recipe_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let content = indoc! {r#"
            ---
            servings: 4
            time: 30 min
            cuisine: Italian
            ---

            Test recipe content"#};
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", content);

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let metadata = &recipe.metadata;

        assert_eq!(metadata.get("servings").unwrap().as_i64().unwrap(), 4);
        assert_eq!(metadata.get("time").unwrap().as_str().unwrap(), "30 min");
        assert_eq!(
            metadata.get("cuisine").unwrap().as_str().unwrap(),
            "Italian"
        );
    }

    #[test]
    fn test_recipe_content_access() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let content = indoc! {r#"
            ---
            servings: 4
            ---

            Add @salt{1%tsp} and @pepper{1%tsp}"#};
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", content);

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();

        // Test that content is accessible
        assert_eq!(recipe.content().unwrap(), content);

        // Test that metadata is parsed
        assert_eq!(recipe.metadata().servings().unwrap(), 4);
    }

    #[test]
    fn test_recipe_equality() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let path1 = create_test_recipe(
            &temp_dir_path,
            "recipe1",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );
        let path2 = create_test_recipe(
            &temp_dir_path,
            "recipe2",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );

        let recipe1 = RecipeEntry::from_path(path1.clone()).unwrap();
        let recipe2 = RecipeEntry::from_path(path1).unwrap();
        let recipe3 = RecipeEntry::from_path(path2).unwrap();

        // Compare paths
        assert_eq!(recipe1.path(), recipe2.path());
        assert_ne!(recipe1.path(), recipe3.path());
    }

    #[test]
    fn test_invalid_recipe_path() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let invalid_path = temp_dir_path.join("nonexistent.cook");

        let result = RecipeEntry::from_path(invalid_path);
        assert!(result.is_err());
    }

    #[test]
    fn test_find_title_image_no_image() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");
        assert!(find_title_image(&recipe_path).is_none());
    }

    #[test]
    fn test_find_title_image_all_extensions() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Test each supported extension
        for ext in ["jpg", "jpeg", "png", "webp"] {
            // Clean up any previous test images
            for old_ext in ["jpg", "jpeg", "png", "webp"] {
                let _ = std::fs::remove_file(recipe_path.with_extension(old_ext));
            }

            let image_path = create_test_image(&temp_dir_path, "test_recipe", ext);
            let found = find_title_image(&recipe_path);

            assert!(found.is_some(), "Failed to find image with extension {ext}");
            assert_eq!(found.unwrap(), image_path);
        }
    }

    #[test]
    fn test_find_title_image_multiple_images() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create images with different extensions
        let jpg_path = create_test_image(&temp_dir_path, "test_recipe", "jpg");
        let _png_path = create_test_image(&temp_dir_path, "test_recipe", "png");
        let _webp_path = create_test_image(&temp_dir_path, "test_recipe", "webp");

        // Should return the first matching extension (jpg)
        let found_image = find_title_image(&recipe_path);
        assert!(found_image.is_some());
        assert_eq!(found_image.unwrap(), jpg_path);
    }

    #[test]
    fn test_recipe_from_content() {
        let content = indoc! {r#"
            ---
            title: Test Recipe
            servings: 4
            ---

            Test recipe content from string"#};

        let recipe =
            RecipeEntry::from_content(content.to_string(), Some("my_recipe".to_string())).unwrap();
        assert_eq!(recipe.name().as_ref().unwrap(), "Test Recipe"); // Title takes precedence
        assert!(recipe.path().is_none());
        assert!(recipe.title_image().is_none());
        assert_eq!(recipe.content().unwrap(), content);
        assert_eq!(recipe.metadata().servings().unwrap(), 4);
    }

    #[test]
    fn test_recipe_with_metadata_image() {
        let content = indoc! {r#"
            ---
            title: Test Recipe
            image: https://example.com/recipe.jpg
            ---

            Test recipe content"#};

        let recipe = RecipeEntry::from_content(content.to_string(), None).unwrap();
        assert_eq!(
            recipe.title_image().as_ref().unwrap(),
            "https://example.com/recipe.jpg"
        );
    }

    #[test]
    fn test_recipe_with_metadata_images_array() {
        let content = indoc! {r#"
            ---
            title: Test Recipe
            images:
              - https://example.com/recipe1.jpg
              - https://example.com/recipe2.jpg
            ---

            Test recipe content"#};

        let recipe = RecipeEntry::from_content(content.to_string(), None).unwrap();
        // Should return the first image from the array
        assert_eq!(
            recipe.title_image().as_ref().unwrap(),
            "https://example.com/recipe1.jpg"
        );
    }

    #[test]
    fn test_recipe_with_metadata_picture() {
        let content = indoc! {r#"
            ---
            title: Test Recipe
            picture: https://example.com/pic.png
            ---

            Test recipe content"#};

        let recipe = RecipeEntry::from_content(content.to_string(), None).unwrap();
        assert_eq!(
            recipe.title_image().as_ref().unwrap(),
            "https://example.com/pic.png"
        );
    }

    #[test]
    fn test_recipe_with_metadata_pictures_array() {
        let content = indoc! {r#"
            ---
            title: Test Recipe
            pictures:
              - https://example.com/pic1.png
              - https://example.com/pic2.png
            ---

            Test recipe content"#};

        let recipe = RecipeEntry::from_content(content.to_string(), None).unwrap();
        assert_eq!(
            recipe.title_image().as_ref().unwrap(),
            "https://example.com/pic1.png"
        );
    }

    #[test]
    fn test_recipe_from_content_no_title() {
        let content = indoc! {r#"
            ---
            servings: 2
            ---

            Test recipe content"#};

        let recipe =
            RecipeEntry::from_content(content.to_string(), Some("content_recipe".to_string()))
                .unwrap();
        assert_eq!(recipe.name().as_ref().unwrap(), "content_recipe");
        assert!(recipe.path().is_none());
    }

    #[test]
    fn test_recipe_from_content_no_name() {
        let content = "Just recipe content";

        let recipe = RecipeEntry::from_content(content.to_string(), None).unwrap();
        assert!(recipe.name().is_none());
        assert!(recipe.path().is_none());
        assert!(recipe.file_name().is_none());
    }

    #[test]
    #[ignore]
    fn test_find_title_image_case_sensitivity() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create an image with uppercase extension
        let image_path = temp_dir_path.join("test_recipe.JPG");
        File::create(&image_path).unwrap();
        let found_image = find_title_image(&recipe_path);

        // Should find the image with uppercase extension
        assert!(found_image.is_some());
    }

    // ========== Tests for StepImageCollection ==========

    #[test]
    fn test_step_image_collection_empty() {
        let collection = StepImageCollection::default();
        assert!(collection.is_empty());
        assert_eq!(collection.count(), 0);
        assert_eq!(collection.get(0, 1), None);
    }

    #[test]
    fn test_step_image_collection_get_zero_step() {
        let mut collection = StepImageCollection::default();
        collection
            .images
            .entry(0)
            .or_insert_with(HashMap::new)
            .insert(0, "test.jpg".to_string());

        // Step 0 should return None (steps are one-indexed)
        assert_eq!(collection.get(0, 0), None);
    }

    #[test]
    fn test_recipe_with_linear_step_images() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create step images: Recipe.1.jpg, Recipe.3.jpg, Recipe.5.jpg
        create_test_image(&temp_dir_path, "test_recipe.1", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.3", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.5", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert!(!images.is_empty());
        assert_eq!(images.count(), 3);

        // Verify images are stored at correct indices (one-indexed access, zero-indexed storage)
        assert!(images.get(0, 1).is_some()); // Recipe.1.jpg at [0][0]
        assert!(images.get(0, 2).is_none()); // No Recipe.2.jpg
        assert!(images.get(0, 3).is_some()); // Recipe.3.jpg at [0][2]
        assert!(images.get(0, 5).is_some()); // Recipe.5.jpg at [0][4]

        // Verify actual paths
        let img1 = images.get(0, 1).unwrap();
        assert!(img1.contains("test_recipe.1.jpg"));
    }

    #[test]
    fn test_recipe_with_section_step_images() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create section-step images: Recipe.2.4.jpg, Recipe.1.1.jpg
        create_test_image(&temp_dir_path, "test_recipe.2.4", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.1.1", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert!(!images.is_empty());
        assert_eq!(images.count(), 2);

        // Recipe.2.4.jpg should be at section 2, step 4 (stored at [1][3])
        assert!(images.get(2, 4).is_some());
        let img = images.get(2, 4).unwrap();
        assert!(img.contains("test_recipe.2.4.jpg"));

        // Recipe.1.1.jpg should be at section 1, step 1 (stored at [0][0])
        assert!(images.get(1, 1).is_some());
        let img = images.get(1, 1).unwrap();
        assert!(img.contains("test_recipe.1.1.jpg"));
    }

    #[test]
    fn test_recipe_with_mixed_image_types() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create mixed images: title, linear step, and section-step
        create_test_image(&temp_dir_path, "test_recipe", "jpg"); // title
        create_test_image(&temp_dir_path, "test_recipe.2", "jpg"); // linear step 2
        create_test_image(&temp_dir_path, "test_recipe.2.4", "jpg"); // section 2, step 4

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();

        // Title image should work
        assert!(recipe.title_image().is_some());

        // Step images should work
        let images = recipe.step_images();
        assert_eq!(images.count(), 2);

        // Recipe.2.jpg is stored at [0][1] (section 0 = linear)
        assert!(images.get(0, 2).is_some());

        // Recipe.2.4.jpg is stored at [1][3] (section 2, step 4)
        assert!(images.get(2, 4).is_some());
    }

    #[test]
    fn test_recipe_step_images_all_extensions() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create images with different extensions
        create_test_image(&temp_dir_path, "test_recipe.1", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.2", "jpeg");
        create_test_image(&temp_dir_path, "test_recipe.3", "png");
        create_test_image(&temp_dir_path, "test_recipe.4", "webp");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert_eq!(images.count(), 4);
        assert!(images.get(0, 1).unwrap().ends_with(".jpg"));
        assert!(images.get(0, 2).unwrap().ends_with(".jpeg"));
        assert!(images.get(0, 3).unwrap().ends_with(".png"));
        assert!(images.get(0, 4).unwrap().ends_with(".webp"));
    }

    #[test]
    fn test_recipe_step_image_extension_priority() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create multiple extensions for same step - jpg should take priority
        create_test_image(&temp_dir_path, "test_recipe.1", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.1", "png");
        create_test_image(&temp_dir_path, "test_recipe.1", "webp");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert_eq!(images.count(), 1);
        assert!(images.get(0, 1).unwrap().ends_with(".jpg"));
    }

    #[test]
    fn test_recipe_from_content_no_step_images() {
        let content = indoc! {r#"
            ---
            servings: 4
            ---

            Test recipe content"#};

        let recipe = RecipeEntry::from_content(content.to_string(), None).unwrap();
        let images = recipe.step_images();

        assert!(images.is_empty());
        assert_eq!(images.count(), 0);
    }

    #[test]
    fn test_recipe_no_step_images() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Only create title image, no step images
        create_test_image(&temp_dir_path, "test_recipe", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert!(images.is_empty());
        assert_eq!(images.count(), 0);
    }

    #[test]
    fn test_recipe_step_images_with_gaps() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create non-consecutive step images
        create_test_image(&temp_dir_path, "test_recipe.1", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.7", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.15", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert_eq!(images.count(), 3);
        assert!(images.get(0, 1).is_some());
        assert!(images.get(0, 2).is_none());
        assert!(images.get(0, 7).is_some());
        assert!(images.get(0, 15).is_some());
    }

    #[test]
    fn test_direct_hashmap_iteration() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        create_test_image(&temp_dir_path, "test_recipe.1", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.2", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        // Test direct HashMap access
        if let Some(section_steps) = images.images.get(&0) {
            assert_eq!(section_steps.len(), 2);
            assert!(section_steps.contains_key(&0)); // Recipe.1.jpg
            assert!(section_steps.contains_key(&1)); // Recipe.2.jpg
        } else {
            panic!("Section 0 should exist");
        }
    }

    #[test]
    fn test_parse_image_numbers_valid() {
        use std::path::PathBuf;

        // Test single number
        let path = PathBuf::from("Recipe.3.jpg");
        let result = parse_image_numbers(&path, "Recipe", "jpg");
        assert_eq!(result, Some(vec![3]));

        // Test two numbers
        let path = PathBuf::from("Recipe.2.4.jpg");
        let result = parse_image_numbers(&path, "Recipe", "jpg");
        assert_eq!(result, Some(vec![2, 4]));
    }

    #[test]
    fn test_parse_image_numbers_invalid() {
        use std::path::PathBuf;

        // Invalid: zero
        let path = PathBuf::from("Recipe.0.jpg");
        let result = parse_image_numbers(&path, "Recipe", "jpg");
        assert_eq!(result, None);

        // Invalid: non-numeric
        let path = PathBuf::from("Recipe.invalid.jpg");
        let result = parse_image_numbers(&path, "Recipe", "jpg");
        assert_eq!(result, None);

        // Invalid: three numbers
        let path = PathBuf::from("Recipe.1.2.3.jpg");
        let result = parse_image_numbers(&path, "Recipe", "jpg");
        assert_eq!(result, None);
    }

    // ========== Tests for extract_recipe_references ==========

    #[test]
    fn test_extract_recipe_references_simple() {
        let content = "Pour @./sauces/Hollandaise{150%g} over the eggs.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs, vec!["./sauces/Hollandaise"]);
    }

    #[test]
    fn test_extract_recipe_references_multiple() {
        let content = "Serve @./sauces/Hollandaise{150%g} with @./sides/Asparagus{200%g}.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"./sauces/Hollandaise".to_string()));
        assert!(refs.contains(&"./sides/Asparagus".to_string()));
    }

    #[test]
    fn test_extract_recipe_references_no_refs() {
        let content = "Add @salt{1%tsp} and @pepper{1%tsp}.";
        let refs = extract_recipe_references(content);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_recipe_references_no_quantity() {
        let content = "Serve with @./sauces/Hollandaise over eggs.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs, vec!["./sauces/Hollandaise"]);
    }

    #[test]
    fn test_extract_recipe_references_deduplicates() {
        let content = "Use @./base/Stock{100%ml} twice and @./base/Stock{200%ml} again.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs, vec!["./base/Stock"]);
    }

    // ========== Tests for related_files ==========

    #[test]
    fn test_related_files_empty() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "simple", "Just a recipe");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();
        assert!(files.is_empty());
    }

    #[test]
    fn test_related_files_with_title_image() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "pasta", "Make pasta");
        let image_path = create_test_image(&temp_dir_path, "pasta", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], image_path);
    }

    #[test]
    fn test_related_files_with_step_images() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "pasta", "Make pasta");
        create_test_image(&temp_dir_path, "pasta.1", "jpg");
        create_test_image(&temp_dir_path, "pasta.2", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_related_files_content_based_returns_empty() {
        let recipe = RecipeEntry::from_content("Just content".to_string(), None).unwrap();
        let files = recipe.related_files();
        assert!(files.is_empty());
    }

    #[test]
    fn test_related_files_with_referenced_recipe() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // Create a subdirectory for the referenced recipe
        let sauces_dir = temp_dir_path.join("sauces");
        std::fs::create_dir_all(&sauces_dir).unwrap();

        // Create the referenced recipe with its own image
        create_test_recipe(&sauces_dir, "Hollandaise", "Melt @butter{100%g}");
        create_test_image(&sauces_dir, "Hollandaise", "jpg");

        // Create the main recipe that references it
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "Eggs Benedict",
            "Pour @./sauces/Hollandaise{150%g} over eggs.",
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Should include: sauces/Hollandaise.cook + sauces/Hollandaise.jpg
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.as_str().ends_with("Hollandaise.cook")));
        assert!(files.iter().any(|f| f.as_str().ends_with("Hollandaise.jpg")));
    }

    #[test]
    fn test_related_files_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let base_dir = temp_dir_path.join("base");
        std::fs::create_dir_all(&base_dir).unwrap();

        let sauces_dir = temp_dir_path.join("sauces");
        std::fs::create_dir_all(&sauces_dir).unwrap();

        // base/Stock.cook (leaf - no references)
        create_test_recipe(&base_dir, "Stock", "Simmer @bones{500%g}");
        create_test_image(&base_dir, "Stock", "png");

        // sauces/Gravy.cook -> references base/Stock
        create_test_recipe(
            &sauces_dir,
            "Gravy",
            "Add @../base/Stock{200%ml} and thicken.",
        );

        // Main recipe -> references sauces/Gravy
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "Roast Dinner",
            "Serve with @./sauces/Gravy{100%ml}.",
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Should include:
        // - sauces/Gravy.cook (direct reference)
        // - base/Stock.cook (transitive reference from Gravy)
        // - base/Stock.png (image of Stock)
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|f| f.as_str().ends_with("Gravy.cook")));
        assert!(files.iter().any(|f| f.as_str().ends_with("Stock.cook")));
        assert!(files.iter().any(|f| f.as_str().ends_with("Stock.png")));
    }

    #[test]
    fn test_related_files_circular_reference() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // Recipe A references Recipe B, Recipe B references Recipe A
        create_test_recipe(
            &temp_dir_path,
            "RecipeA",
            "Use @./RecipeB{100%g} as base.",
        );
        create_test_recipe(
            &temp_dir_path,
            "RecipeB",
            "Use @./RecipeA{50%g} as topping.",
        );

        let recipe_path = temp_dir_path.join("RecipeA.cook");
        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Should include RecipeB.cook but not loop infinitely
        assert_eq!(files.len(), 1);
        assert!(files.iter().any(|f| f.as_str().ends_with("RecipeB.cook")));
    }

    #[test]
    fn test_related_files_missing_reference() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "incomplete",
            "Use @./nonexistent/Recipe{100%g}.",
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Missing references are silently skipped
        assert!(files.is_empty());
    }
}
