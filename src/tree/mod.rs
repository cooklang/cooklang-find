//! Recipe tree building for directory hierarchies.
//!
//! This module provides functionality to build hierarchical tree structures
//! that represent the organization of recipe files within a directory tree.

use crate::model::{RecipeEntry, RecipeEntryError};
use crate::walk::glob_visible;
use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

mod listing;
mod model;
pub use listing::{count_recipes, list_dir, DirEntry, DirListing};
pub use model::RecipeTree;

/// Errors that can occur when building a recipe tree.
#[derive(Error, Debug)]
pub enum TreeError {
    #[error("Directory does not exist: {0}")]
    DirectoryNotFound(String),

    #[error("Path is not a directory: {0}")]
    NotADirectory(String),

    #[error("Failed to read directory: {0}")]
    GlobError(#[from] glob::GlobError),

    #[error("Failed to read directory: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to create glob pattern: {0}")]
    PatternError(#[from] glob::PatternError),

    #[error("Failed to process recipe: {0}")]
    RecipeEntryError(#[from] RecipeEntryError),

    #[error("Failed to strip prefix from path: {0}")]
    StripPrefixError(String),
}

/// Builds a hierarchical tree structure of all recipes in a directory.
///
/// This function recursively scans the specified directory and all its
/// subdirectories for .cook and .menu files, organizing them into a tree
/// structure that mirrors the filesystem hierarchy.
///
/// # Arguments
///
/// * `base_dir` - The root directory to build the tree from
///
/// # Returns
///
/// Returns a `RecipeTree` representing the directory structure with all
/// recipes loaded, or a `TreeError` if the operation fails.
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::build_tree;
/// use camino::Utf8Path;
///
/// // Build a tree of all recipes in a directory
/// let tree = build_tree(Utf8Path::new("./recipes"))?;
///
/// // Access recipes in the tree
/// for (name, node) in &tree.children {
///     if let Some(recipe) = &node.recipe {
///         println!("Found recipe: {}", name);
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
///
/// This opens every recipe in the subtree. To show one folder at a time,
/// use [`list_dir`] instead.
pub fn build_tree<P: AsRef<Utf8Path>>(base_dir: P) -> Result<RecipeTree, TreeError> {
    let base_dir = base_dir.as_ref();

    // Check if directory exists
    if !base_dir.exists() {
        return Err(TreeError::DirectoryNotFound(base_dir.to_string()));
    }
    if !base_dir.is_dir() {
        return Err(TreeError::NotADirectory(base_dir.to_string()));
    }

    let base_name = base_dir
        .file_name()
        .map(|n| n.to_string())
        .unwrap_or_else(|| String::from("./"));

    let mut root = RecipeTree::new(base_name, base_dir.to_path_buf());

    // First, find all .cook and .menu files in this directory and subdirectories
    let patterns = vec![
        base_dir.join("**/*.cook").to_string(),
        base_dir.join("**/*.menu").to_string(),
    ];

    for pattern in patterns {
        for entry in glob_visible(&pattern)? {
            let path = entry?;
            let path = Utf8PathBuf::from_path_buf(path).map_err(|_| {
                TreeError::StripPrefixError("Path contains invalid UTF-8".to_string())
            })?;
            let recipe = match RecipeEntry::from_path(path.clone()) {
                Ok(r) => r,
                Err(_) => continue, // Skip files whose content isn't available (e.g. iCloud)
            };

            // Calculate the relative path from the base directory
            let rel_path = path
                .strip_prefix(base_dir)
                .map_err(|_| TreeError::StripPrefixError(path.to_string()))?;

            // Build the tree structure
            let mut current = &mut root;
            let components: Vec<_> = rel_path
                .parent()
                .map(|p| p.components().collect())
                .unwrap_or_default();

            // Create directory nodes
            for component in components {
                let name = component.to_string();
                let path = current.path.join(&name);
                make_room_for_folder(current, &name);
                current = current
                    .children
                    .entry(name.clone())
                    .or_insert_with(|| RecipeTree::new(name, path));
            }

            // Add the recipe as a leaf node, keyed by its name unless a folder
            // or another recipe in this directory already has it.
            let name = recipe.name().clone().unwrap();
            let key = if current.children.contains_key(&name) {
                file_name_key(&path)
            } else {
                name.clone()
            };

            current
                .children
                .insert(key, RecipeTree::new_with_recipe(name, path, recipe));
        }
    }

    Ok(root)
}

/// A folder is always keyed by its own name. A recipe that took the name
/// first (`Week.menu` found before `Week/Monday.menu`) moves to its file
/// name, the key it would have had if found second. Without this, the folder's
/// recipes were filed under the recipe node, and a listing that tells folders
/// from recipes by their children lost one or the other.
fn make_room_for_folder(dir: &mut RecipeTree, name: &str) {
    if dir
        .children
        .get(name)
        .is_some_and(|node| node.recipe.is_some())
    {
        let recipe = dir.children.remove(name).expect("checked above");
        dir.children.insert(file_name_key(&recipe.path), recipe);
    }
}

/// The key of a recipe whose name is taken: its file name, which no other
/// entry of the same directory can have.
fn file_name_key(path: &Utf8Path) -> String {
    path.file_name().unwrap_or_default().to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Utf8Path, name: &str, content: &str) -> Utf8PathBuf {
        let path = dir.join(format!("{name}.cook"));
        fs::write(&path, content).unwrap();
        path
    }

    fn create_test_image(dir: &Utf8Path, name: &str, ext: &str) -> Utf8PathBuf {
        let path = dir.join(format!("{name}.{ext}"));
        fs::write(&path, "dummy image content").unwrap();
        path
    }

    #[test]
    fn test_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let tree = build_tree(&temp_dir_path).unwrap();

        assert_eq!(tree.name, temp_dir_path.file_name().unwrap().to_string());
        assert_eq!(tree.path, temp_dir_path);
        assert!(tree.recipe.is_none());
        assert!(tree.children.is_empty());
    }

    #[test]
    fn test_single_recipe() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(
            &temp_dir_path,
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );

        let tree = build_tree(&temp_dir_path).unwrap();

        assert_eq!(tree.children.len(), 1);
        let recipe_node = tree.children.get("pancakes").unwrap();
        assert_eq!(recipe_node.name, "pancakes");
        assert!(recipe_node.recipe.is_some());
        assert!(recipe_node.children.is_empty());
    }

    #[test]
    fn test_recipe_with_image() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(
            &temp_dir_path,
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );
        create_test_image(&temp_dir_path, "pancakes", "jpg");

        let tree = build_tree(&temp_dir_path).unwrap();

        let recipe_node = tree.children.get("pancakes").unwrap();
        assert!(recipe_node.recipe.as_ref().unwrap().title_image().is_some());
    }

    #[test]
    fn test_nested_directories() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // Create nested directory structure
        let breakfast_dir = temp_dir_path.join("breakfast");
        let dessert_dir = temp_dir_path.join("dessert");
        fs::create_dir_all(&breakfast_dir).unwrap();
        fs::create_dir_all(&dessert_dir).unwrap();

        // Add recipes
        create_test_recipe(
            &breakfast_dir,
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );
        create_test_recipe(
            &breakfast_dir,
            "waffles",
            indoc! {r#"
                ---
                servings: 2
                ---

                Make waffles"#},
        );
        create_test_recipe(
            &dessert_dir,
            "cake",
            indoc! {r#"
                ---
                servings: 8
                ---

                Bake cake"#},
        );

        let tree = build_tree(&temp_dir_path).unwrap();

        assert_eq!(tree.children.len(), 2);

        // Check breakfast directory
        let breakfast = tree.children.get("breakfast").unwrap();
        assert_eq!(breakfast.name, "breakfast");
        assert!(breakfast.recipe.is_none());
        assert_eq!(breakfast.children.len(), 2);
        assert!(breakfast.children.contains_key("pancakes"));
        assert!(breakfast.children.contains_key("waffles"));

        // Check dessert directory
        let dessert = tree.children.get("dessert").unwrap();
        assert_eq!(dessert.name, "dessert");
        assert!(dessert.recipe.is_none());
        assert_eq!(dessert.children.len(), 1);
        assert!(dessert.children.contains_key("cake"));
    }

    #[test]
    fn test_deeply_nested_recipe() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let deep_path = temp_dir_path.join("a/b/c/d");
        fs::create_dir_all(&deep_path).unwrap();

        create_test_recipe(
            &deep_path,
            "deep_recipe",
            indoc! {r#"
                ---
                servings: 1
                ---

                Deep recipe"#},
        );

        let tree = build_tree(&temp_dir_path).unwrap();

        let a = tree.children.get("a").unwrap();
        let b = a.children.get("b").unwrap();
        let c = b.children.get("c").unwrap();
        let d = c.children.get("d").unwrap();
        let recipe = d.children.get("deep_recipe").unwrap();

        assert!(recipe.recipe.is_some());
        assert_eq!(recipe.name, "deep_recipe");
    }

    #[test]
    fn test_recipe_count_includes_nested_directories() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // One recipe directly in the directory, two more in a subdirectory.
        let bakery = temp_dir_path.join("bakery");
        let breads = bakery.join("breads");
        fs::create_dir_all(&breads).unwrap();
        create_test_recipe(&bakery, "pita", "Bake pita");
        create_test_recipe(&breads, "sourdough", "Bake sourdough");
        create_test_recipe(&breads, "focaccia", "Bake focaccia");

        let tree = build_tree(&temp_dir_path).unwrap();

        let bakery_node = tree.children.get("bakery").unwrap();
        assert_eq!(bakery_node.recipe_count(), 3);
        assert_eq!(
            bakery_node.children.get("breads").unwrap().recipe_count(),
            2
        );
        assert_eq!(tree.recipe_count(), 3);
    }

    #[test]
    fn test_recipe_count_of_directory_holding_only_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // "household" holds no recipe of its own, only subdirectories that do.
        let household = temp_dir_path.join("household");
        fs::create_dir_all(household.join("laundry")).unwrap();
        fs::create_dir_all(household.join("floors")).unwrap();
        create_test_recipe(&household.join("laundry"), "detergent", "Mix detergent");
        create_test_recipe(&household.join("floors"), "cleaner", "Mix cleaner");
        create_test_recipe(&household.join("floors"), "polish", "Mix polish");

        let tree = build_tree(&temp_dir_path).unwrap();

        let household_node = tree.children.get("household").unwrap();
        assert!(household_node.recipe.is_none());
        assert_eq!(household_node.recipe_count(), 3);
    }

    #[test]
    fn test_recipe_count_of_a_recipe_node_is_one() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&temp_dir_path, "pancakes", "Make pancakes");

        let tree = build_tree(&temp_dir_path).unwrap();

        assert_eq!(tree.children.get("pancakes").unwrap().recipe_count(), 1);
    }

    #[test]
    fn test_invalid_directory() {
        let result = build_tree(Utf8Path::new("/nonexistent/directory"));
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Directory does not exist"));
    }

    #[test]
    fn test_recipe_tree_new() {
        let tree = RecipeTree::new("test".to_string(), Utf8PathBuf::from("/test/path"));

        assert_eq!(tree.name, "test");
        assert_eq!(tree.path, Utf8PathBuf::from("/test/path"));
        assert!(tree.recipe.is_none());
        assert!(tree.children.is_empty());
    }

    #[test]
    fn test_recipe_tree_new_with_recipe() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "test_recipe",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe"#},
        );

        let recipe = RecipeEntry::from_path(recipe_path.clone()).unwrap();
        let tree = RecipeTree::new_with_recipe("test_recipe".to_string(), recipe_path, recipe);

        assert_eq!(tree.name, "test_recipe");
        assert!(tree.recipe.is_some());
        assert!(tree.children.is_empty());
    }

    #[test]
    fn test_skips_hidden_files_and_directories() {
        // macOS writes AppleDouble `._<name>` companions next to every file it
        // touches on a non-HFS share; they end in `.cook` but are binary
        // resource forks, not recipes. https://github.com/cooklang/cookcli/issues/555
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(&temp_dir_path, "pancakes", "Make @pancakes{}");
        fs::write(
            temp_dir_path.join("._pancakes.cook"),
            b"\x00\x05\x16\x07\x00\x02\x00\x00Mac OS X        ",
        )
        .unwrap();
        let hidden_dir = temp_dir_path.join(".Trashes");
        fs::create_dir_all(&hidden_dir).unwrap();
        create_test_recipe(&hidden_dir, "deleted", "Make @toast{}");

        let tree = build_tree(&temp_dir_path).unwrap();

        let names: Vec<_> = tree.children.keys().cloned().collect();
        assert_eq!(names, vec!["pancakes".to_string()]);
    }

    #[test]
    fn test_finds_recipes_with_non_lowercase_extensions() {
        // https://github.com/cooklang/cooklang-find/issues/11
        let temp_dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(root.join("U/Sub/Deep")).unwrap();
        fs::write(root.join("U/Sub/Deep/x.COOK"), "Mix.").unwrap();
        fs::write(root.join("Week.Menu"), "= Day 1").unwrap();
        fs::write(root.join("pie.Cook"), "Bake.").unwrap();

        let tree = build_tree(&root).unwrap();

        let mut names: Vec<_> = tree.children.keys().cloned().collect();
        names.sort();
        assert_eq!(names, vec!["U", "Week", "pie"]);
        assert!(tree.children["Week"].recipe.as_ref().unwrap().is_menu());
        assert_eq!(tree.recipe_count(), 3);
    }

    /// The folder `name` under `dir`, holding the recipe `inside`.
    fn assert_folder(dir: &RecipeTree, name: &str, inside: &str) {
        let folder = &dir.children[name];
        assert!(folder.recipe.is_none(), "{name} must be the folder");
        assert!(
            folder.children[inside].recipe.is_some(),
            "{name}/{inside} must be listed in the folder"
        );
    }

    /// The recipe keyed `key` under `dir`, still named `name`.
    fn assert_recipe(dir: &RecipeTree, key: &str, name: &str) {
        let recipe = &dir.children[key];
        assert_eq!(recipe.name, name);
        assert!(recipe.recipe.is_some(), "{key} must be the recipe");
        assert!(
            recipe.children.is_empty(),
            "nothing may be filed under {key}"
        );
    }

    #[test]
    fn test_folder_and_menu_of_the_same_name_are_both_kept() {
        // A menu found next to the folder its days are in.
        let temp_dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(root.join("Plans/Week")).unwrap();
        fs::write(root.join("Plans/Week.menu"), "= Day 1").unwrap();
        fs::write(root.join("Plans/Week/Monday.menu"), "= Day 1").unwrap();

        let tree = build_tree(&root).unwrap();

        let plans = &tree.children["Plans"];
        assert_eq!(plans.children.len(), 2);
        assert_folder(plans, "Week", "Monday");
        assert_recipe(plans, "Week.menu", "Week");
        assert_eq!(tree.recipe_count(), 2);
    }

    #[test]
    fn test_folder_keeps_its_name_whichever_is_found_first() {
        let temp_dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        // `.cook` files are found before `.menu` files: `Soup.cook` is a recipe
        // before the folder `Soup/` exists, `Stew/Beef.cook` makes the folder
        // `Stew/` before the menu `Stew.menu` is found.
        fs::create_dir_all(root.join("Soup")).unwrap();
        fs::create_dir_all(root.join("Stew")).unwrap();
        fs::write(root.join("Soup.cook"), "Blend.").unwrap();
        fs::write(root.join("Soup/Week.menu"), "= Day 1").unwrap();
        fs::write(root.join("Stew/Beef.cook"), "Simmer.").unwrap();
        fs::write(root.join("Stew.menu"), "= Day 1").unwrap();

        let tree = build_tree(&root).unwrap();

        assert_folder(&tree, "Soup", "Week");
        assert_recipe(&tree, "Soup.cook", "Soup");
        assert_folder(&tree, "Stew", "Beef");
        assert_recipe(&tree, "Stew.menu", "Stew");
        assert_eq!(tree.children.len(), 4);
        assert_eq!(tree.recipe_count(), 4);
    }

    #[test]
    fn test_recipe_and_menu_of_the_same_name_are_both_kept() {
        let temp_dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        fs::write(root.join("Pasta.cook"), "Boil.").unwrap();
        fs::write(root.join("Pasta.menu"), "= Day 1").unwrap();

        let tree = build_tree(&root).unwrap();

        assert_recipe(&tree, "Pasta", "Pasta");
        assert!(!tree.children["Pasta"].recipe.as_ref().unwrap().is_menu());
        assert_recipe(&tree, "Pasta.menu", "Pasta");
        assert!(tree.children["Pasta.menu"]
            .recipe
            .as_ref()
            .unwrap()
            .is_menu());
        assert_eq!(tree.recipe_count(), 2);
    }

    #[test]
    fn test_folder_recipe_and_menu_of_the_same_name_are_all_kept() {
        // As a user's collection had it: only the menu was left, the folder
        // filed under the recipe and both replaced by the menu.
        let temp_dir = TempDir::new().unwrap();
        let root = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        fs::create_dir_all(root.join("Plats")).unwrap();
        fs::write(root.join("Plats/Poulet.cook"), "Roast.").unwrap();
        fs::write(root.join("Plats.cook"), "Simmer.").unwrap();
        fs::write(root.join("Plats.menu"), "= Day 1").unwrap();

        let tree = build_tree(&root).unwrap();

        assert_eq!(tree.children.len(), 3);
        assert_folder(&tree, "Plats", "Poulet");
        assert_recipe(&tree, "Plats.cook", "Plats");
        assert!(!tree.children["Plats.cook"]
            .recipe
            .as_ref()
            .unwrap()
            .is_menu());
        assert_recipe(&tree, "Plats.menu", "Plats");
        assert!(tree.children["Plats.menu"]
            .recipe
            .as_ref()
            .unwrap()
            .is_menu());
        assert_eq!(tree.recipe_count(), 3);
    }
}
