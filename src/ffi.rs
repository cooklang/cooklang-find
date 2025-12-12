//! UniFFI bindings for cross-platform support (iOS, Android).
//!
//! This module provides FFI-safe types and functions for use with UniFFI.
//! Complex types are converted to simpler representations suitable for FFI.

use crate::fetcher::{get_recipe_str, FetchError};
use crate::model::{Metadata, RecipeEntry, RecipeEntryError, StepImageCollection};
use crate::search::{search as search_internal, SearchError};
use crate::tree::{build_tree as build_tree_internal, RecipeTree, TreeError};
use camino::Utf8Path;
use std::sync::Arc;

/// FFI-safe error type that wraps all possible errors.
#[derive(Debug, uniffi::Error, thiserror::Error)]
pub enum CooklangError {
    #[error("Recipe not found: {message}")]
    NotFound { message: String },

    #[error("IO error: {message}")]
    IoError { message: String },

    #[error("Parse error: {message}")]
    ParseError { message: String },

    #[error("Invalid path: {message}")]
    InvalidPath { message: String },

    #[error("Search error: {message}")]
    SearchError { message: String },

    #[error("Tree error: {message}")]
    TreeError { message: String },
}

impl From<FetchError> for CooklangError {
    fn from(e: FetchError) -> Self {
        match e {
            FetchError::IoError(e) => CooklangError::IoError {
                message: e.to_string(),
            },
            FetchError::RecipeEntryError(e) => e.into(),
            FetchError::InvalidPath(p) => CooklangError::NotFound {
                message: format!("Recipe not found: {}", p),
            },
        }
    }
}

impl From<RecipeEntryError> for CooklangError {
    fn from(e: RecipeEntryError) -> Self {
        match e {
            RecipeEntryError::IoError(e) => CooklangError::IoError {
                message: e.to_string(),
            },
            RecipeEntryError::InvalidPath(p) => CooklangError::InvalidPath {
                message: p.to_string(),
            },
            RecipeEntryError::ParseError(msg) => CooklangError::ParseError { message: msg },
            RecipeEntryError::MetadataError(msg) => CooklangError::ParseError { message: msg },
        }
    }
}

impl From<SearchError> for CooklangError {
    fn from(e: SearchError) -> Self {
        CooklangError::SearchError {
            message: e.to_string(),
        }
    }
}

impl From<TreeError> for CooklangError {
    fn from(e: TreeError) -> Self {
        CooklangError::TreeError {
            message: e.to_string(),
        }
    }
}

/// A key-value pair for metadata entries.
#[derive(Debug, Clone, uniffi::Record)]
pub struct MetadataEntry {
    pub key: String,
    pub value: String,
}

/// FFI-safe representation of recipe metadata.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiMetadata {
    /// Recipe title if present
    pub title: Option<String>,
    /// Number of servings if present
    pub servings: Option<i64>,
    /// List of tags
    pub tags: Vec<String>,
    /// Primary image URL if present
    pub image_url: Option<String>,
    /// All metadata as JSON string for complex access
    pub raw_json: String,
}

impl From<&Metadata> for FfiMetadata {
    fn from(m: &Metadata) -> Self {
        // Convert the internal data to JSON for complex access
        let raw_json = serde_json::to_string(&m).unwrap_or_default();

        FfiMetadata {
            title: m.title().map(|s| s.to_string()),
            servings: m.servings(),
            tags: m.tags(),
            image_url: m.image_url(),
            raw_json,
        }
    }
}

/// A step image entry mapping section and step to an image path.
#[derive(Debug, Clone, uniffi::Record)]
pub struct StepImageEntry {
    /// Section number (0 for linear recipes, 1+ for sectioned recipes)
    pub section: u32,
    /// Step number (1-indexed)
    pub step: u32,
    /// Path to the image
    pub image_path: String,
}

/// FFI-safe representation of step images.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiStepImages {
    /// List of all step images
    pub images: Vec<StepImageEntry>,
    /// Total count of images
    pub count: u32,
}

impl From<&StepImageCollection> for FfiStepImages {
    fn from(c: &StepImageCollection) -> Self {
        let mut images = Vec::new();

        for (section_idx, steps) in &c.images {
            for (step_idx, path) in steps {
                // Convert back from zero-indexed storage to one-indexed API
                let section = if *section_idx == 0 {
                    0 // Linear recipe
                } else {
                    (*section_idx + 1) as u32 // Sectioned recipe
                };
                let step = (*step_idx + 1) as u32;

                images.push(StepImageEntry {
                    section,
                    step,
                    image_path: path.clone(),
                });
            }
        }

        // Sort by section then step for predictable ordering
        images.sort_by(|a, b| {
            let section_cmp = a.section.cmp(&b.section);
            if section_cmp == std::cmp::Ordering::Equal {
                a.step.cmp(&b.step)
            } else {
                section_cmp
            }
        });

        FfiStepImages {
            count: images.len() as u32,
            images,
        }
    }
}

/// FFI-safe representation of a recipe entry.
///
/// This is the main type for representing recipes across the FFI boundary.
#[derive(uniffi::Object)]
pub struct FfiRecipeEntry {
    inner: RecipeEntry,
}

#[uniffi::export]
impl FfiRecipeEntry {
    /// Returns the name of the recipe.
    pub fn name(&self) -> Option<String> {
        self.inner.name().clone()
    }

    /// Returns the file path if this recipe is backed by a file.
    pub fn path(&self) -> Option<String> {
        self.inner.path().map(|p| p.to_string())
    }

    /// Returns the file name if this recipe is backed by a file.
    pub fn file_name(&self) -> Option<String> {
        self.inner.file_name()
    }

    /// Returns the full content of the recipe.
    pub fn content(&self) -> Result<String, CooklangError> {
        self.inner.content().map_err(|e| e.into())
    }

    /// Returns the recipe's metadata.
    pub fn metadata(&self) -> FfiMetadata {
        FfiMetadata::from(self.inner.metadata())
    }

    /// Returns the recipe's tags.
    pub fn tags(&self) -> Vec<String> {
        self.inner.tags()
    }

    /// Returns the URL or path to the recipe's title image.
    pub fn title_image(&self) -> Option<String> {
        self.inner.title_image().clone()
    }

    /// Returns all step images for the recipe.
    pub fn step_images(&self) -> FfiStepImages {
        FfiStepImages::from(self.inner.step_images())
    }

    /// Returns true if this is a menu file (.menu) rather than a recipe (.cook).
    pub fn is_menu(&self) -> bool {
        self.inner.is_menu()
    }

    /// Gets a step image by section and step number.
    ///
    /// For linear recipes (no sections), use section = 0.
    /// Steps are one-indexed (first step is 1).
    pub fn get_step_image(&self, section: u32, step: u32) -> Option<String> {
        self.inner
            .step_images()
            .get(section as usize, step as usize)
            .cloned()
    }

    /// Gets a specific metadata value by key as a JSON string.
    pub fn get_metadata_value(&self, key: String) -> Option<String> {
        self.inner
            .metadata()
            .get(&key)
            .map(|v| serde_json::to_string(v).unwrap_or_default())
    }
}

impl FfiRecipeEntry {
    fn new(entry: RecipeEntry) -> Self {
        FfiRecipeEntry { inner: entry }
    }
}

/// FFI-safe representation of a tree node.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiTreeNode {
    /// Name of the node (directory or recipe name)
    pub name: String,
    /// Full path to this node
    pub path: String,
    /// True if this node has a recipe
    pub has_recipe: bool,
    /// Names of child nodes
    pub children: Vec<String>,
}

/// FFI-safe representation of a recipe tree.
#[derive(uniffi::Object)]
pub struct FfiRecipeTree {
    inner: RecipeTree,
}

#[uniffi::export]
impl FfiRecipeTree {
    /// Returns the root node information.
    pub fn root(&self) -> FfiTreeNode {
        tree_to_node(&self.inner)
    }

    /// Returns all nodes in the tree as a flat list.
    pub fn all_nodes(&self) -> Vec<FfiTreeNode> {
        let mut nodes = Vec::new();
        collect_nodes(&self.inner, &mut nodes);
        nodes
    }

    /// Returns all recipes in the tree.
    pub fn all_recipes(&self) -> Vec<Arc<FfiRecipeEntry>> {
        let mut recipes = Vec::new();
        collect_recipes(&self.inner, &mut recipes);
        recipes
    }

    /// Gets a child node by name from the root.
    pub fn get_child(&self, name: String) -> Option<FfiTreeNode> {
        self.inner.children.get(&name).map(tree_to_node)
    }

    /// Gets the recipe at the root level if present.
    pub fn recipe(&self) -> Option<Arc<FfiRecipeEntry>> {
        self.inner
            .recipe
            .as_ref()
            .map(|r| Arc::new(FfiRecipeEntry::new(r.clone())))
    }

    /// Gets a recipe by path components (e.g., ["breakfast", "pancakes"]).
    pub fn get_recipe_at_path(&self, path: Vec<String>) -> Option<Arc<FfiRecipeEntry>> {
        let mut current = &self.inner;
        for component in &path {
            current = current.children.get(component)?;
        }
        current
            .recipe
            .as_ref()
            .map(|r| Arc::new(FfiRecipeEntry::new(r.clone())))
    }
}

fn tree_to_node(tree: &RecipeTree) -> FfiTreeNode {
    FfiTreeNode {
        name: tree.name.clone(),
        path: tree.path.to_string(),
        has_recipe: tree.recipe.is_some(),
        children: tree.children.keys().cloned().collect(),
    }
}

fn collect_nodes(tree: &RecipeTree, nodes: &mut Vec<FfiTreeNode>) {
    nodes.push(tree_to_node(tree));
    for child in tree.children.values() {
        collect_nodes(child, nodes);
    }
}

fn collect_recipes(tree: &RecipeTree, recipes: &mut Vec<Arc<FfiRecipeEntry>>) {
    if let Some(recipe) = &tree.recipe {
        recipes.push(Arc::new(FfiRecipeEntry::new(recipe.clone())));
    }
    for child in tree.children.values() {
        collect_recipes(child, recipes);
    }
}

// ============================================================================
// Exported FFI Functions
// ============================================================================

/// Loads a recipe by name from the specified directories.
///
/// Searches through the provided directories in order for a recipe file
/// matching the given name. Automatically handles .cook and .menu extensions.
///
/// # Arguments
/// * `base_dirs` - List of directory paths to search
/// * `name` - Recipe name to search for (with or without extension)
///
/// # Returns
/// The recipe if found, or an error.
#[uniffi::export]
pub fn get_recipe(
    base_dirs: Vec<String>,
    name: String,
) -> Result<Arc<FfiRecipeEntry>, CooklangError> {
    let entry = get_recipe_str(base_dirs, &name)?;
    Ok(Arc::new(FfiRecipeEntry::new(entry)))
}

/// Creates a recipe from file content.
///
/// Useful for creating recipes from sources other than files,
/// such as network responses or programmatically generated content.
///
/// # Arguments
/// * `content` - The full recipe content including any YAML frontmatter
/// * `name` - Optional name for the recipe
///
/// # Returns
/// The recipe entry, or an error if parsing fails.
#[uniffi::export]
pub fn recipe_from_content(
    content: String,
    name: Option<String>,
) -> Result<Arc<FfiRecipeEntry>, CooklangError> {
    let entry = RecipeEntry::from_content(content, name)?;
    Ok(Arc::new(FfiRecipeEntry::new(entry)))
}

/// Creates a recipe from a file path.
///
/// # Arguments
/// * `path` - The path to the recipe file
///
/// # Returns
/// The recipe entry, or an error if loading fails.
#[uniffi::export]
pub fn recipe_from_path(path: String) -> Result<Arc<FfiRecipeEntry>, CooklangError> {
    let entry = RecipeEntry::from_path(path.into())?;
    Ok(Arc::new(FfiRecipeEntry::new(entry)))
}

/// Searches for recipes matching a query string.
///
/// Performs full-text search across recipe filenames and contents
/// in the specified directory and subdirectories.
///
/// # Arguments
/// * `base_dir` - Root directory to search in
/// * `query` - Search query (can contain multiple space-separated terms)
///
/// # Returns
/// List of matching recipes sorted by relevance.
#[uniffi::export]
pub fn search(base_dir: String, query: String) -> Result<Vec<Arc<FfiRecipeEntry>>, CooklangError> {
    let results = search_internal(Utf8Path::new(&base_dir), &query)?;
    Ok(results
        .into_iter()
        .map(|r| Arc::new(FfiRecipeEntry::new(r)))
        .collect())
}

/// Builds a hierarchical tree of all recipes in a directory.
///
/// Recursively scans the directory for .cook and .menu files,
/// organizing them into a tree structure mirroring the filesystem.
///
/// # Arguments
/// * `base_dir` - Root directory to build the tree from
///
/// # Returns
/// The recipe tree, or an error.
#[uniffi::export]
pub fn build_tree(base_dir: String) -> Result<Arc<FfiRecipeTree>, CooklangError> {
    let tree = build_tree_internal(&base_dir)?;
    Ok(Arc::new(FfiRecipeTree { inner: tree }))
}

/// Returns the library version.
#[uniffi::export]
pub fn library_version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &str, name: &str, content: &str) -> String {
        let path = format!("{}/{}.cook", dir, name);
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_recipe_from_content() {
        let content = indoc! {r#"
            ---
            title: Test Recipe
            servings: 4
            tags: [breakfast, easy]
            ---

            Add @eggs{2} and mix"#};

        let recipe = recipe_from_content(content.to_string(), None).unwrap();
        assert_eq!(recipe.name(), Some("Test Recipe".to_string()));
        assert_eq!(recipe.metadata().servings, Some(4));
        assert_eq!(recipe.tags(), vec!["breakfast", "easy"]);
    }

    #[test]
    fn test_search_recipes() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();

        create_test_recipe(
            temp_path,
            "pancakes",
            indoc! {r#"
            ---
            title: Fluffy Pancakes
            ---

            Mix and cook"#},
        );

        create_test_recipe(
            temp_path,
            "waffles",
            indoc! {r#"
            ---
            title: Crispy Waffles
            ---

            Make waffles"#},
        );

        let results = search(temp_path.to_string(), "pancakes".to_string()).unwrap();
        assert_eq!(results.len(), 1);
        assert_eq!(results[0].name(), Some("Fluffy Pancakes".to_string()));
    }

    #[test]
    fn test_build_tree() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();

        // Create nested structure
        let breakfast_dir = format!("{}/breakfast", temp_path);
        fs::create_dir_all(&breakfast_dir).unwrap();

        create_test_recipe(
            &breakfast_dir,
            "pancakes",
            indoc! {r#"
            ---
            title: Pancakes
            ---

            Make pancakes"#},
        );

        let tree = build_tree(temp_path.to_string()).unwrap();
        let nodes = tree.all_nodes();
        assert!(nodes.len() >= 2); // At least root and breakfast directory

        let recipes = tree.all_recipes();
        assert_eq!(recipes.len(), 1);
    }

    #[test]
    fn test_step_images_conversion() {
        use std::collections::HashMap;

        let mut collection = StepImageCollection::default();
        collection.images.insert(0, HashMap::new());
        collection
            .images
            .get_mut(&0)
            .unwrap()
            .insert(0, "/path/to/image1.jpg".to_string());
        collection
            .images
            .get_mut(&0)
            .unwrap()
            .insert(2, "/path/to/image3.jpg".to_string());

        let ffi_images = FfiStepImages::from(&collection);
        assert_eq!(ffi_images.count, 2);
        assert_eq!(ffi_images.images[0].section, 0);
        assert_eq!(ffi_images.images[0].step, 1);
        assert_eq!(ffi_images.images[1].step, 3);
    }

    #[test]
    fn test_library_version() {
        let version = library_version();
        assert!(!version.is_empty());
        assert_eq!(version, env!("CARGO_PKG_VERSION"));
    }
}
