use super::companions::{scan, Slot};
use super::lossy::{lines_lossy, read_to_string_lossy};
use super::metadata::{extract_and_parse_metadata, Metadata};
use crate::menu::Menu;
use crate::reference::{extract_references, resolve_reference};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::sync::OnceLock;
use thiserror::Error;

/// A recipe's step and section images, by one-based step number.
///
/// - `RecipeName.N.ext` is step N of a recipe without sections: `linear[N]`
/// - `RecipeName.S.N.ext` is section S, step N: `sectioned[S][N]`
///
/// One path per slot; when several extensions exist for a step the first in
/// [`IMAGE_EXTENSIONS`](crate::IMAGE_EXTENSIONS) order wins.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct StepImageCollection {
    /// `Name.N.ext`, keyed by one-based step.
    pub linear: BTreeMap<usize, String>,
    /// `Name.S.N.ext`, keyed by one-based section, then one-based step.
    pub sectioned: BTreeMap<usize, BTreeMap<usize, String>>,
}

impl StepImageCollection {
    /// Returns true if there are no images at all.
    pub fn is_empty(&self) -> bool {
        self.linear.is_empty() && self.sectioned.is_empty()
    }

    /// Total number of images across both kinds.
    pub fn count(&self) -> usize {
        self.linear.len() + self.sectioned.values().map(BTreeMap::len).sum::<usize>()
    }

    /// The image for a step: `section == 0` reads the linear images,
    /// `section >= 1` the sectioned ones. `step == 0` is always `None`.
    ///
    /// # Examples
    /// ```
    /// # use cooklang_find::StepImageCollection;
    /// # let images = StepImageCollection::default();
    /// // Recipe.3.jpg
    /// let img = images.get(0, 3);
    /// // Recipe.2.4.jpg
    /// let img = images.get(2, 4);
    /// ```
    pub fn get(&self, section: usize, step: usize) -> Option<&String> {
        if step == 0 {
            return None;
        }
        if section == 0 {
            self.linear.get(&step)
        } else {
            self.sectioned.get(&section)?.get(&step)
        }
    }

    /// Builds the collection from a sorted companion scan; the first file
    /// per slot wins.
    pub(crate) fn from_companions(companions: &[(Slot, Utf8PathBuf)]) -> Self {
        let mut collection = StepImageCollection::default();
        for (slot, path) in companions {
            match *slot {
                Slot::Title => {}
                Slot::Linear(step) => {
                    collection
                        .linear
                        .entry(step)
                        .or_insert_with(|| path.to_string());
                }
                Slot::Sectioned(section, step) => {
                    collection
                        .sectioned
                        .entry(section)
                        .or_default()
                        .entry(step)
                        .or_insert_with(|| path.to_string());
                }
            }
        }
        collection
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
#[serde(from = "RecipeEntryData", into = "RecipeEntryData")]
pub struct RecipeEntry {
    /// Source of the recipe (path or content)
    source: RecipeSource,
    /// Metadata; read on first access for entries from `from_path_lazy`
    metadata: OnceLock<Metadata>,

    /// Cached name of the recipe (from file stem, title, or provided name)
    name: OnceLock<Option<String>>,
    /// Optional path or URL to the title image
    title_image: OnceLock<Option<String>>,
    /// Cached title and step images, from one folder listing
    images: OnceLock<Box<Images>>,
    /// Whether this is a menu file (*.menu) rather than a regular recipe
    is_menu: OnceLock<bool>,
}

/// Serialized form of a [`RecipeEntry`]: its source and metadata.
#[derive(Serialize, Deserialize)]
struct RecipeEntryData {
    source: RecipeSource,
    metadata: Metadata,
}

impl From<RecipeEntry> for RecipeEntryData {
    fn from(entry: RecipeEntry) -> Self {
        let metadata = entry.metadata().clone();
        RecipeEntryData {
            source: entry.source,
            metadata,
        }
    }
}

impl From<RecipeEntryData> for RecipeEntry {
    fn from(data: RecipeEntryData) -> Self {
        RecipeEntry::new(data.source, OnceLock::from(data.metadata))
    }
}

impl Clone for RecipeEntry {
    fn clone(&self) -> Self {
        // Metadata is kept if it was loaded; derived fields are recomputed
        // on demand.
        RecipeEntry::new(self.source.clone(), self.metadata.clone())
    }
}

/// Image lookups, all from one listing of the recipe's folder.
#[derive(Debug)]
struct Images {
    /// Every companion with its slot, sorted; see `companions::scan`.
    companions: Vec<(Slot, Utf8PathBuf)>,
    /// One path per step slot, built from `companions`.
    steps: StepImageCollection,
}

impl RecipeEntry {
    fn new(source: RecipeSource, metadata: OnceLock<Metadata>) -> Self {
        RecipeEntry {
            source,
            metadata,
            name: OnceLock::new(),
            title_image: OnceLock::new(),
            images: OnceLock::new(),
            is_menu: OnceLock::new(),
        }
    }

    /// The image cache, filled on first use; empty for content-based entries.
    fn images(&self) -> &Images {
        self.images.get_or_init(|| {
            let companions = match &self.source {
                RecipeSource::Path { path } => scan(path),
                RecipeSource::Content { .. } => Vec::new(),
            };
            let steps = StepImageCollection::from_companions(&companions);
            Box::new(Images { companions, steps })
        })
    }

    /// The companion scan, done once.
    fn companions(&self) -> &[(Slot, Utf8PathBuf)] {
        &self.images().companions
    }

    /// Creates a new `RecipeEntry` from a file path.
    ///
    /// Reads the recipe file, extracts metadata from YAML frontmatter,
    /// and creates a fully initialized recipe entry. Bytes that are not valid
    /// UTF-8 are decoded lossily rather than refused; see the `lossy` module
    /// for why.
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
        let metadata = read_metadata(&path)?;
        Ok(RecipeEntry::new(
            RecipeSource::Path { path },
            OnceLock::from(metadata),
        ))
    }

    /// Creates a `RecipeEntry` for a file without opening it.
    ///
    /// The frontmatter is read on the first call that needs it
    /// ([`metadata`](Self::metadata), [`name`](Self::name),
    /// [`title_image`](Self::title_image), [`tags`](Self::tags)) and cached.
    /// [`path`](Self::path), [`file_name`](Self::file_name) and
    /// [`is_menu`](Self::is_menu) never touch the file.
    ///
    /// Use this when listing many files of which only a few will be shown,
    /// or when opening a file is expensive (iCloud / File Provider
    /// placeholders). If the file can't be read when its metadata is first
    /// needed, the metadata is empty, as for a file without frontmatter.
    ///
    /// # Arguments
    ///
    /// * `path` - The path to the recipe file (.cook or .menu)
    pub fn from_path_lazy(path: Utf8PathBuf) -> Self {
        RecipeEntry::new(RecipeSource::Path { path }, OnceLock::new())
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

        Ok(RecipeEntry::new(
            RecipeSource::Content { content, name },
            OnceLock::from(metadata),
        ))
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
            if let Some(title) = self.metadata().title() {
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
    /// Supported image extensions: jpg, jpeg, png, webp, heic, in any ASCII case.
    ///
    /// The result is cached after the first call.
    pub fn title_image(&self) -> &Option<String> {
        self.title_image.get_or_init(|| {
            // First check metadata for image URLs
            if let Some(url) = self.metadata().image_url() {
                return Some(url);
            }

            // For path-based recipes, check for file-based images
            self.companions()
                .iter()
                .find(|(slot, _)| *slot == Slot::Title)
                .map(|(_, path)| path.to_string())
        })
    }

    /// Returns the full content of the recipe.
    ///
    /// For path-based recipes, this reads the file from disk, decoding bytes
    /// that are not valid UTF-8 lossily rather than refusing the file; see the
    /// `lossy` module for why.
    /// For content-based recipes, this returns the stored content.
    ///
    /// # Errors
    ///
    /// Returns `RecipeEntryError::IoError` if the file cannot be read
    /// (only applicable for path-based recipes).
    pub fn content(&self) -> Result<String, RecipeEntryError> {
        match &self.source {
            RecipeSource::Path { path } => {
                let file = File::open(path).map_err(RecipeEntryError::IoError)?;
                read_to_string_lossy(&mut BufReader::new(file)).map_err(RecipeEntryError::IoError)
            }
            RecipeSource::Content { content, .. } => Ok(content.clone()),
        }
    }

    /// Returns a reference to the recipe's metadata.
    ///
    /// The metadata contains all fields from the YAML frontmatter,
    /// providing access to both standard fields (title, servings, tags)
    /// and any custom fields defined in the recipe.
    ///
    /// For an entry from [`from_path_lazy`](Self::from_path_lazy), the first
    /// call reads the file; if it can't be read, the metadata is empty.
    pub fn metadata(&self) -> &Metadata {
        self.metadata.get_or_init(|| match &self.source {
            RecipeSource::Path { path } => read_metadata(path).unwrap_or_default(),
            // Content entries always parse their metadata when created.
            RecipeSource::Content { .. } => Metadata::default(),
        })
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
        self.metadata().tags()
    }

    /// Checks if this entry represents a menu file.
    ///
    /// Returns `true` if the file has a .menu extension (in any case, e.g.
    /// `.MENU`), `false` otherwise (including content-based recipes).
    pub fn is_menu(&self) -> bool {
        *self.is_menu.get_or_init(|| match &self.source {
            RecipeSource::Path { path } => crate::walk::has_extension(path, "menu"),
            RecipeSource::Content { .. } => false,
        })
    }

    /// Parses this entry as a [`Menu`].
    ///
    /// Returns `None` when the entry is not a `.menu` file, including for
    /// content-backed entries, which are never menus.
    ///
    /// # Errors
    ///
    /// Returns `RecipeEntryError::IoError` if the file cannot be read.
    pub fn menu(&self) -> Option<Result<Menu, RecipeEntryError>> {
        if !self.is_menu() {
            return None;
        }
        let fallback_name = self.name().clone().unwrap_or_default();
        Some(
            self.content()
                .map(|content| Menu::parse(&content, &fallback_name)),
        )
    }

    /// Returns all step and section images for the recipe.
    ///
    /// Images follow the Cooklang naming convention (one-indexed):
    /// - `RecipeName.N.ext`: Step N in linear recipe (read with section 0)
    /// - `RecipeName.S.N.ext`: Section S, step N within section
    ///
    /// All step and section numbers are one-indexed (first step/section is 1).
    ///
    /// Supported extensions: jpg, jpeg, png, webp, heic (in priority order), any ASCII case
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
    /// // Iterate every image
    /// for (step, img) in &images.linear {
    ///     println!("Step {step}: {img}");
    /// }
    /// for (section, steps) in &images.sectioned {
    ///     for (step, img) in steps {
    ///         println!("Section {section}, step {step}: {img}");
    ///     }
    /// }
    /// # Ok::<(), Box<dyn std::error::Error>>(())
    /// ```
    pub fn step_images(&self) -> &StepImageCollection {
        &self.images().steps
    }

    /// The title image and every step image next to this recipe; see
    /// [`companion_files`](crate::companion_files). Empty for content-based
    /// entries. Never a referenced recipe.
    pub fn companion_files(&self) -> Vec<Utf8PathBuf> {
        self.companions()
            .iter()
            .map(|(_, path)| path.clone())
            .collect()
    }

    /// Every file this recipe depends on: its companion files (title and
    /// step images, every extension variant), the recipes and menus it
    /// references, and, recursively, theirs.
    ///
    /// References resolve against `base_dir`, the collection root, with
    /// [`resolve_reference`](crate::resolve_reference). References inside
    /// frontmatter or comments are ignored. Missing references are skipped;
    /// cycles are broken. Empty for content-based entries.
    pub fn related_files(&self, base_dir: &Utf8Path) -> Vec<Utf8PathBuf> {
        let RecipeSource::Path { path } = &self.source else {
            return Vec::new();
        };
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        collect_related_files(base_dir, path, &mut visited, &mut result);
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

/// Reads and parses the YAML frontmatter of the file at `path`.
fn read_metadata(path: &Utf8Path) -> Result<Metadata, RecipeEntryError> {
    let file = File::open(path).map_err(RecipeEntryError::IoError)?;
    extract_and_parse_metadata(
        lines_lossy(BufReader::new(file)).map(|r| r.map_err(RecipeEntryError::IoError)),
    )
}

/// Canonical form for the visited set, or the path itself when it cannot
/// be canonicalised.
fn canonical(path: &Utf8Path) -> Utf8PathBuf {
    match std::fs::canonicalize(path) {
        Ok(p) => Utf8PathBuf::from_path_buf(p)
            .unwrap_or_else(|p| Utf8PathBuf::from(p.to_string_lossy().into_owned())),
        Err(_) => path.to_path_buf(),
    }
}

/// Recursively collects the files related to `recipe_path`: companions,
/// then each resolved reference and its own related files.
fn collect_related_files(
    base_dir: &Utf8Path,
    recipe_path: &Utf8Path,
    visited: &mut HashSet<Utf8PathBuf>,
    result: &mut Vec<Utf8PathBuf>,
) {
    if !visited.insert(canonical(recipe_path)) {
        return;
    }
    result.extend(scan(recipe_path).into_iter().map(|(_, path)| path));

    // Decoded lossily, so a stray non-UTF-8 byte does not hide references.
    let Ok(content) =
        File::open(recipe_path).and_then(|file| read_to_string_lossy(&mut BufReader::new(file)))
    else {
        return;
    };
    for reference in extract_references(&content) {
        let Some(file) = resolve_reference(base_dir, &reference) else {
            continue;
        };
        if !visited.contains(&canonical(&file)) {
            result.push(file.clone());
            collect_related_files(base_dir, &file, visited, result);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs::{self, File};
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
        let metadata = recipe.metadata();

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
        assert!(RecipeEntry::from_path(recipe_path.clone())
            .unwrap()
            .title_image()
            .is_none());
    }

    #[test]
    fn test_find_title_image_all_extensions() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Test each supported extension
        for ext in ["jpg", "jpeg", "png", "webp", "heic"] {
            // Clean up any previous test images
            for old_ext in ["jpg", "jpeg", "png", "webp", "heic"] {
                let _ = std::fs::remove_file(recipe_path.with_extension(old_ext));
            }

            let image_path = create_test_image(&temp_dir_path, "test_recipe", ext);
            let found = RecipeEntry::from_path(recipe_path.clone())
                .unwrap()
                .title_image()
                .clone();

            assert!(found.is_some(), "Failed to find image with extension {ext}");
            assert_eq!(found, Some(image_path.to_string()));
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
        let found_image = RecipeEntry::from_path(recipe_path.clone())
            .unwrap()
            .title_image()
            .clone();
        assert_eq!(found_image, Some(jpg_path.to_string()));
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
    fn test_find_title_image_case_sensitivity() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        // Create an image with uppercase extension
        let image_path = temp_dir_path.join("test_recipe.JPG");
        File::create(&image_path).unwrap();
        let found_image = RecipeEntry::from_path(recipe_path.clone())
            .unwrap()
            .title_image()
            .clone();

        // Should find the image with uppercase extension
        assert!(found_image
            .as_deref()
            .is_some_and(|p| p.ends_with("test_recipe.JPG")));
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
        collection.linear.insert(1, "test.jpg".to_string());

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

        // `get(0, n)` is the linear step image `Recipe.n.jpg`.
        assert!(images.get(0, 1).is_some()); // Recipe.1.jpg
        assert!(images.get(0, 2).is_none()); // No Recipe.2.jpg
        assert!(images.get(0, 3).is_some()); // Recipe.3.jpg
        assert!(images.get(0, 5).is_some()); // Recipe.5.jpg

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

        // `get(2, 4)` → `Recipe.2.4.jpg`
        assert!(images.get(2, 4).is_some());
        let img = images.get(2, 4).unwrap();
        assert!(img.contains("test_recipe.2.4.jpg"));

        // `get(1, 1)` → `Recipe.1.1.jpg`
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

        // `get(0, 2)` → `Recipe.2.jpg` (section 0 is the linear step list)
        assert!(images.get(0, 2).is_some());

        // `get(2, 4)` → `Recipe.2.4.jpg`
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
    fn test_linear_images_iterate_by_step() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");

        create_test_image(&temp_dir_path, "test_recipe.1", "jpg");
        create_test_image(&temp_dir_path, "test_recipe.2", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert_eq!(
            images.linear.keys().copied().collect::<Vec<_>>(),
            vec![1, 2]
        );
    }

    #[test]
    fn test_linear_and_section_one_images_do_not_collide() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");
        let linear = create_test_image(&temp_dir_path, "test_recipe.2", "jpg");
        let sectioned = create_test_image(&temp_dir_path, "test_recipe.1.2", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert_eq!(images.count(), 2);
        assert_eq!(images.get(0, 2), Some(&linear.to_string()));
        assert_eq!(images.get(1, 2), Some(&sectioned.to_string()));
    }

    #[test]
    fn test_companion_files_on_entry() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "pasta", "Make @./Pesto{}");
        create_test_recipe(&temp_dir_path, "Pesto", "Blend");
        let title = create_test_image(&temp_dir_path, "pasta", "jpg");
        let step = create_test_image(&temp_dir_path, "pasta.1", "heic");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        assert_eq!(recipe.companion_files(), vec![title, step]);

        let content = RecipeEntry::from_content("Make".to_string(), None).unwrap();
        assert!(content.companion_files().is_empty());
    }

    // ========== Tests for related_files ==========

    #[test]
    fn test_related_files_empty() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "simple", "Just a recipe");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files(&temp_dir_path);
        assert!(files.is_empty());
    }

    #[test]
    fn test_related_files_with_title_image() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "pasta", "Make pasta");
        let image_path = create_test_image(&temp_dir_path, "pasta", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files(&temp_dir_path);
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
        let files = recipe.related_files(&temp_dir_path);
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_related_files_content_based_returns_empty() {
        let recipe = RecipeEntry::from_content("Just content".to_string(), None).unwrap();
        let files = recipe.related_files(Utf8Path::new("/nowhere"));
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
        let files = recipe.related_files(&temp_dir_path);

        // Should include: sauces/Hollandaise.cook + sauces/Hollandaise.jpg
        assert_eq!(files.len(), 2);
        assert!(files
            .iter()
            .any(|f| f.as_str().ends_with("Hollandaise.cook")));
        assert!(files
            .iter()
            .any(|f| f.as_str().ends_with("Hollandaise.jpg")));
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
            "Add @./base/Stock{200%ml} and thicken.",
        );

        // Main recipe -> references sauces/Gravy
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "Roast Dinner",
            "Serve with @./sauces/Gravy{100%ml}.",
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files(&temp_dir_path);

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
        create_test_recipe(&temp_dir_path, "RecipeA", "Use @./RecipeB{100%g} as base.");
        create_test_recipe(
            &temp_dir_path,
            "RecipeB",
            "Use @./RecipeA{50%g} as topping.",
        );

        let recipe_path = temp_dir_path.join("RecipeA.cook");
        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files(&temp_dir_path);

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
        let files = recipe.related_files(&temp_dir_path);

        // Missing references are silently skipped
        assert!(files.is_empty());
    }

    #[test]
    fn test_related_files_resolve_from_base_dir_not_recipe_folder() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let sub = temp_dir_path.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        let root_b = create_test_recipe(&temp_dir_path, "B", "Root B");
        create_test_recipe(&sub, "B", "Sub B");
        let a = create_test_recipe(&sub, "A", "Use @./B{} here.");

        let recipe = RecipeEntry::from_path(a).unwrap();
        assert_eq!(recipe.related_files(&temp_dir_path), vec![root_b]);
    }

    #[test]
    fn test_related_files_follows_menu_references_and_spaces() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let menu = temp_dir_path.join("Sunday.menu");
        std::fs::write(&menu, "= Day\n- @./Pesto Sauce{}\n").unwrap();
        let pesto = create_test_recipe(&temp_dir_path, "Pesto Sauce", "Blend");
        let plan = create_test_recipe(
            &temp_dir_path,
            "Plan",
            "See @./Sunday{} for @./Pesto Sauce{2}.",
        );

        let recipe = RecipeEntry::from_path(plan).unwrap();
        assert_eq!(recipe.related_files(&temp_dir_path), vec![menu, pesto]);
    }

    #[test]
    fn test_related_files_ignore_commented_out_references() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&temp_dir_path, "Hidden", "x");
        let shown = create_test_recipe(&temp_dir_path, "Shown", "y");
        let plan = create_test_recipe(
            &temp_dir_path,
            "Plan",
            "---\nsource: @./Hidden\n---\n[- @./Hidden{} -] @./Shown{} -- @./Hidden{}",
        );

        let recipe = RecipeEntry::from_path(plan).unwrap();
        assert_eq!(recipe.related_files(&temp_dir_path), vec![shown]);
    }

    fn temp_dir() -> (TempDir, Utf8PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        (temp_dir, path)
    }

    #[test]
    fn lazy_entry_does_not_open_the_file() {
        let (_tmp, dir) = temp_dir();
        let missing = dir.join("Week Plan.menu");

        let entry = RecipeEntry::from_path_lazy(missing.clone());

        assert_eq!(entry.path(), Some(&missing));
        assert_eq!(entry.file_name().as_deref(), Some("Week Plan.menu"));
        assert!(entry.is_menu());
    }

    #[test]
    fn lazy_entry_reads_metadata_on_first_access_and_caches_it() {
        let (_tmp, dir) = temp_dir();
        let path = create_test_recipe(&dir, "pancakes", "---\ntitle: Before\n---\n");
        let entry = RecipeEntry::from_path_lazy(path.clone());

        // Written after the entry was made: the entry hasn't read the file yet.
        fs::write(&path, "---\ntitle: First read\ntags: [quick]\n---\n").unwrap();
        assert_eq!(entry.metadata().title(), Some("First read"));

        // Written after the first access: the cached metadata is kept.
        fs::write(&path, "---\ntitle: Second read\n---\n").unwrap();
        assert_eq!(entry.metadata().title(), Some("First read"));
        assert_eq!(entry.name().as_deref(), Some("First read"));
        assert_eq!(entry.tags(), vec!["quick"]);
    }

    #[test]
    fn lazy_entry_for_unreadable_file_has_empty_metadata() {
        let (_tmp, dir) = temp_dir();
        let entry = RecipeEntry::from_path_lazy(dir.join("gone.cook"));

        assert_eq!(entry.metadata(), &Metadata::default());
        assert_eq!(entry.name().as_deref(), Some("gone"));
        assert_eq!(entry.title_image(), &None);
    }

    #[test]
    fn lazy_entry_title_image_comes_from_metadata() {
        let (_tmp, dir) = temp_dir();
        let path = create_test_recipe(&dir, "cake", "---\nimage: https://x.test/cake.jpg\n---\n");

        let entry = RecipeEntry::from_path_lazy(path);

        assert_eq!(
            entry.title_image().as_deref(),
            Some("https://x.test/cake.jpg")
        );
    }

    #[test]
    fn lazy_entry_serializes_with_its_metadata() {
        let (_tmp, dir) = temp_dir();
        let path = create_test_recipe(&dir, "soup", "---\nservings: 4\n---\n");
        let lazy = RecipeEntry::from_path_lazy(path.clone());
        let eager = RecipeEntry::from_path(path).unwrap();

        let lazy_json = serde_json::to_value(&lazy).unwrap();

        assert_eq!(lazy_json, serde_json::to_value(&eager).unwrap());
        assert_eq!(lazy_json["metadata"]["servings"], 4);
        let back: RecipeEntry = serde_json::from_value(lazy_json).unwrap();
        assert_eq!(back.metadata().servings(), Some(4));
    }

    #[test]
    fn clone_keeps_loaded_metadata() {
        let (_tmp, dir) = temp_dir();
        let path = create_test_recipe(&dir, "stew", "---\ntitle: Stew\n---\n");
        let entry = RecipeEntry::from_path_lazy(path.clone());
        assert_eq!(entry.metadata().title(), Some("Stew"));
        fs::write(&path, "---\ntitle: Changed\n---\n").unwrap();

        assert_eq!(entry.clone().metadata().title(), Some("Stew"));
    }

    #[test]
    fn is_menu_ignores_extension_case() {
        let temp_dir = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let path = dir.join("week.MENU");
        std::fs::write(&path, "= Day 1").unwrap();

        assert!(RecipeEntry::from_path(path).unwrap().is_menu());
    }
}
