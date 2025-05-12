use crate::RecipeEntry;
use glob::glob;
use std::path::Path;
use thiserror::Error;

mod model;
pub use model::RecipeTree;

#[derive(Error, Debug)]
pub enum TreeError {
    #[error("Directory does not exist: {0}")]
    DirectoryNotFound(String),

    #[error("Path is not a directory: {0}")]
    NotADirectory(String),

    #[error("Failed to read directory: {0}")]
    GlobError(#[from] glob::GlobError),

    #[error("Failed to create glob pattern: {0}")]
    PatternError(#[from] glob::PatternError),

    #[error("Failed to process recipe: {0}")]
    RecipeEntryError(#[from] crate::RecipeEntryError),

    #[error("Failed to strip prefix from path: {0}")]
    StripPrefixError(String),
}

/// Build a tree structure of recipes and directories for a given base directory
pub fn build_tree<P: AsRef<Path>>(base_dir: P) -> Result<RecipeTree, TreeError> {
    let base_dir = base_dir.as_ref();

    // Check if directory exists
    if !base_dir.exists() {
        return Err(TreeError::DirectoryNotFound(base_dir.display().to_string()));
    }
    if !base_dir.is_dir() {
        return Err(TreeError::NotADirectory(base_dir.display().to_string()));
    }

    let base_name = base_dir
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| String::from("./"));

    let mut root = RecipeTree::new(base_name, base_dir.to_owned());

    // First, find all .cook files in this directory and subdirectories
    let pattern = base_dir.join("**/*.cook");
    let pattern = pattern.to_string_lossy();

    for entry in glob(&pattern)? {
        let path = entry?;
        let recipe = RecipeEntry::from_path(path.clone())?;

        // Calculate the relative path from the base directory
        let rel_path = path
            .strip_prefix(base_dir)
            .map_err(|_| TreeError::StripPrefixError(path.display().to_string()))?;

        // Build the tree structure
        let mut current = &mut root;
        let components: Vec<_> = rel_path
            .parent()
            .map(|p| p.components().collect())
            .unwrap_or_default();

        // Create directory nodes
        for component in components {
            let name = component.as_os_str().to_string_lossy().into_owned();
            let path = current.path.join(&name);
            current = current
                .children
                .entry(name.clone())
                .or_insert_with(|| RecipeTree::new(name, path));
        }

        // Add the recipe as a leaf node
        let name = recipe.name().clone().unwrap();

        current.children.insert(
            name.clone(),
            RecipeTree::new_with_recipe(name, path, recipe),
        );
    }

    Ok(root)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;
    use std::path::PathBuf;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(format!("{}.cook", name));
        fs::write(&path, content).unwrap();
        path
    }

    fn create_test_image(dir: &Path, name: &str, ext: &str) -> PathBuf {
        let path = dir.join(format!("{}.{}", name, ext));
        fs::write(&path, "dummy image content").unwrap();
        path
    }

    #[test]
    fn test_empty_directory() {
        let temp_dir = TempDir::new().unwrap();
        let tree = build_tree(temp_dir.path()).unwrap();

        assert_eq!(
            tree.name,
            temp_dir.path().file_name().unwrap().to_string_lossy()
        );
        assert_eq!(tree.path, temp_dir.path());
        assert!(tree.recipe.is_none());
        assert!(tree.children.is_empty());
    }

    #[test]
    fn test_single_recipe() {
        let temp_dir = TempDir::new().unwrap();
        create_test_recipe(
            temp_dir.path(),
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );

        let tree = build_tree(temp_dir.path()).unwrap();

        assert_eq!(tree.children.len(), 1);
        let recipe_node = tree.children.get("pancakes").unwrap();
        assert_eq!(recipe_node.name, "pancakes");
        assert!(recipe_node.recipe.is_some());
        assert!(recipe_node.children.is_empty());
    }

    #[test]
    fn test_recipe_with_image() {
        let temp_dir = TempDir::new().unwrap();
        create_test_recipe(
            temp_dir.path(),
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );
        create_test_image(temp_dir.path(), "pancakes", "jpg");

        let tree = build_tree(temp_dir.path()).unwrap();

        let recipe_node = tree.children.get("pancakes").unwrap();
        assert!(recipe_node.recipe.as_ref().unwrap().title_image().is_some());
    }

    #[test]
    fn test_nested_directories() {
        let temp_dir = TempDir::new().unwrap();

        // Create nested directory structure
        let breakfast_dir = temp_dir.path().join("breakfast");
        let dessert_dir = temp_dir.path().join("dessert");
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

        let tree = build_tree(temp_dir.path()).unwrap();

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
        let deep_path = temp_dir.path().join("a/b/c/d");
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

        let tree = build_tree(temp_dir.path()).unwrap();

        let a = tree.children.get("a").unwrap();
        let b = a.children.get("b").unwrap();
        let c = b.children.get("c").unwrap();
        let d = c.children.get("d").unwrap();
        let recipe = d.children.get("deep_recipe").unwrap();

        assert!(recipe.recipe.is_some());
        assert_eq!(recipe.name, "deep_recipe");
    }

    #[test]
    fn test_invalid_directory() {
        let result = build_tree("/nonexistent/directory");
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Directory does not exist"));
    }

    #[test]
    fn test_recipe_tree_new() {
        let tree = RecipeTree::new("test".to_string(), PathBuf::from("/test/path"));

        assert_eq!(tree.name, "test");
        assert_eq!(tree.path, PathBuf::from("/test/path"));
        assert!(tree.recipe.is_none());
        assert!(tree.children.is_empty());
    }

    #[test]
    fn test_recipe_tree_new_with_recipe() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(
            temp_dir.path(),
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
}
