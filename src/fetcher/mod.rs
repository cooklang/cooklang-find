use crate::recipe::Recipe;
use std::path::{Path, PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("Failed to read recipe file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse recipe: {0}")]
    ParseError(String),

    #[error("Invalid recipe path: {0}")]
    InvalidPath(PathBuf),
}

/// Find a recipe by name
pub fn get_recipe<P: AsRef<Path>>(
    base_dirs: impl IntoIterator<Item = P>,
    name: P,
) -> Result<Option<Recipe>, FetchError> {
    let name = name.as_ref();

    for base_dir in base_dirs {
        let recipe_path = base_dir.as_ref().join(format!("{}.cook", name.display()));
        if recipe_path.exists() {
            return Recipe::new(recipe_path)
                .map(Some)
                .map_err(|e| FetchError::ParseError(e.to_string()));
        }
    }

    Ok(None)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Path, name: &str, content: &str) -> PathBuf {
        let path = dir.join(format!("{}.cook", name));
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_get_recipe_found() {
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

        let result = get_recipe([temp_dir.path()], Path::new("pancakes")).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "pancakes");
    }

    #[test]
    fn test_get_recipe_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let result = get_recipe([temp_dir.path()], Path::new("nonexistent")).unwrap();
        assert!(result.is_none());
    }

    #[test]
    fn test_get_recipe_multiple_directories() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();

        create_test_recipe(
            dir2.path(),
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );

        let result = get_recipe([dir1.path(), dir2.path()], Path::new("pancakes")).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "pancakes");
    }

    #[test]
    fn test_get_recipe_first_directory_priority() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();

        create_test_recipe(
            dir1.path(),
            "pancakes",
            indoc! {r#"
                ---
                servings: 2
                ---

                Dir1 pancakes"#},
        );
        create_test_recipe(
            dir2.path(),
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Dir2 pancakes"#},
        );

        let result = get_recipe([dir1.path(), dir2.path()], Path::new("pancakes")).unwrap();
        assert!(result.is_some());
        let recipe = result.unwrap();
        assert_eq!(recipe.name, "pancakes");
        assert!(recipe.path.starts_with(dir1.path())); // Should find the recipe in the first directory
    }

    #[test]
    fn test_get_recipe_invalid_directory() {
        let result = get_recipe([Path::new("/nonexistent/directory")], Path::new("recipe"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_recipe_empty_directories() {
        let result = get_recipe(std::iter::empty::<PathBuf>(), PathBuf::from("recipe"));
        assert!(result.is_ok());
        assert!(result.unwrap().is_none());
    }

    #[test]
    fn test_get_recipe_with_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let sub_dir = temp_dir.path().join("breakfast");
        fs::create_dir_all(&sub_dir).unwrap();

        create_test_recipe(
            &sub_dir,
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );

        // Should not find recipe in subdirectory when searching base directory
        let result = get_recipe([temp_dir.path()], Path::new("pancakes")).unwrap();
        assert!(result.is_none());

        // Should find recipe when searching subdirectory directly
        let result = get_recipe([sub_dir], Path::new("pancakes").to_path_buf()).unwrap();
        assert!(result.is_some());
        assert_eq!(result.unwrap().name, "pancakes");
    }
}
