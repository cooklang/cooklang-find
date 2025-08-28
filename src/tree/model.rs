use crate::model::RecipeEntry;
use camino::Utf8PathBuf;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Represents a node in a hierarchical recipe directory tree.
///
/// Each node can represent either:
/// - A directory (with `recipe` = None and potentially non-empty `children`)
/// - A recipe file (with `recipe` = Some and empty `children`)
///
/// The tree structure mirrors the filesystem hierarchy, making it easy to
/// navigate and display recipes organized by their directory structure.
///
/// # Fields
///
/// * `name` - The name of this node (directory or recipe name)
/// * `path` - The full filesystem path to this node
/// * `recipe` - Optional recipe data if this node represents a recipe file
/// * `children` - Child nodes indexed by their names
#[derive(Debug, Serialize, Deserialize)]
pub struct RecipeTree {
    /// Name of the current node (directory name or recipe name)
    pub name: String,
    /// Full path to this node
    pub path: Utf8PathBuf,
    /// If this is a recipe, contains the Recipe struct
    pub recipe: Option<RecipeEntry>,
    /// Child directories and recipes
    pub children: HashMap<String, RecipeTree>,
}

impl RecipeTree {
    pub(crate) fn new(name: String, path: Utf8PathBuf) -> Self {
        RecipeTree {
            name,
            path,
            recipe: None,
            children: HashMap::new(),
        }
    }

    pub(crate) fn new_with_recipe(name: String, path: Utf8PathBuf, recipe: RecipeEntry) -> Self {
        RecipeTree {
            name,
            path,
            recipe: Some(recipe),
            children: HashMap::new(),
        }
    }
}
