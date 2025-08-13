use crate::RecipeEntry;
use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum FetchError {
    #[error("Failed to read recipe file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to parse recipe: {0}")]
    RecipeEntryError(#[from] crate::RecipeEntryError),

    #[error("Invalid recipe path: {0}")]
    InvalidPath(Utf8PathBuf),
}

/// Find a recipe by name
pub fn get_recipe<P: AsRef<Utf8Path>>(
    base_dirs: impl IntoIterator<Item = P>,
    name: P,
) -> Result<RecipeEntry, FetchError> {
    let name = name.as_ref();

    for base_dir in base_dirs {
        let recipe_path = if name.extension().is_some() {
            // If the name already has an extension, use it as-is
            base_dir.as_ref().join(name)
        } else {
            // Only append .cook if there's no extension
            base_dir.as_ref().join(format!("{name}.cook"))
        };

        if recipe_path.exists() {
            return RecipeEntry::from_path(recipe_path).map_err(FetchError::RecipeEntryError);
        }
    }

    Err(FetchError::InvalidPath(name.to_path_buf()))
}

/// Find a recipe by name using string paths (convenience function)
pub fn get_recipe_str(
    base_dirs: impl IntoIterator<Item = impl AsRef<str>>,
    name: &str,
) -> Result<RecipeEntry, FetchError> {
    let base_dirs: Vec<Utf8PathBuf> = base_dirs
        .into_iter()
        .map(|s| Utf8PathBuf::from(s.as_ref()))
        .collect();
    let name = Utf8PathBuf::from(name);
    get_recipe(base_dirs, name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Utf8Path, name: &str, content: &str) -> Utf8PathBuf {
        let path = if name.ends_with(".cook") {
            dir.join(name)
        } else {
            dir.join(format!("{name}.cook"))
        };
        fs::write(&path, content).unwrap();
        path
    }

    #[test]
    fn test_get_recipe_found() {
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

        let result = get_recipe([&temp_dir_path], &Utf8PathBuf::from("pancakes")).unwrap();
        assert_eq!(result.name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_get_recipe_not_found() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let result = get_recipe([&temp_dir_path], &Utf8PathBuf::from("nonexistent"));
        assert!(matches!(result, Err(FetchError::InvalidPath(_))));
    }

    #[test]
    fn test_get_recipe_multiple_directories() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let dir1_path = Utf8PathBuf::from_path_buf(dir1.path().to_path_buf()).unwrap();
        let dir2_path = Utf8PathBuf::from_path_buf(dir2.path().to_path_buf()).unwrap();

        create_test_recipe(
            &dir2_path,
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );

        let result = get_recipe([&dir1_path, &dir2_path], &Utf8PathBuf::from("pancakes")).unwrap();
        assert_eq!(result.name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_get_recipe_first_directory_priority() {
        let dir1 = TempDir::new().unwrap();
        let dir2 = TempDir::new().unwrap();
        let dir1_path = Utf8PathBuf::from_path_buf(dir1.path().to_path_buf()).unwrap();
        let dir2_path = Utf8PathBuf::from_path_buf(dir2.path().to_path_buf()).unwrap();

        create_test_recipe(
            &dir1_path,
            "pancakes",
            indoc! {r#"
                ---
                servings: 2
                ---

                Dir1 pancakes"#},
        );
        create_test_recipe(
            &dir2_path,
            "pancakes",
            indoc! {r#"
                ---
                servings: 4
                ---

                Dir2 pancakes"#},
        );

        let result = get_recipe([&dir1_path, &dir2_path], &Utf8PathBuf::from("pancakes")).unwrap();
        assert_eq!(result.name().as_ref().unwrap(), "pancakes");
        assert!(result.path().as_ref().unwrap().starts_with(&dir1_path)); // Should find the recipe in the first directory
    }

    #[test]
    fn test_get_recipe_invalid_directory() {
        let result = get_recipe(
            [Utf8PathBuf::from("/nonexistent/directory")],
            Utf8PathBuf::from("recipe"),
        );
        assert!(matches!(result, Err(FetchError::InvalidPath(_))));
    }

    #[test]
    fn test_get_recipe_empty_directories() {
        let result = get_recipe(
            std::iter::empty::<Utf8PathBuf>(),
            Utf8PathBuf::from("recipe"),
        );
        assert!(matches!(result, Err(FetchError::InvalidPath(_))));
    }

    #[test]
    fn test_get_recipe_with_subdirectories() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let sub_dir = temp_dir_path.join("breakfast");
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
        let result = get_recipe([&temp_dir_path], &Utf8PathBuf::from("pancakes"));
        assert!(matches!(result, Err(FetchError::InvalidPath(_))));

        // Should find recipe when searching subdirectory directly
        let result = get_recipe([&sub_dir], &Utf8PathBuf::from("pancakes")).unwrap();
        assert_eq!(result.name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_get_recipe_with_existing_extension() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        create_test_recipe(
            &temp_dir_path,
            "pancakes.cook",
            indoc! {r#"
                ---
                servings: 4
                ---

                Make pancakes"#},
        );

        // Should find recipe when name already includes .cook extension
        let result = get_recipe([&temp_dir_path], &Utf8PathBuf::from("pancakes.cook")).unwrap();
        assert_eq!(result.name().as_ref().unwrap(), "pancakes");
    }

    #[test]
    fn test_get_recipe_with_menu_extension() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // Create a .menu file
        let menu_path = temp_dir_path.join("weekly.menu");
        fs::write(
            &menu_path,
            indoc! {r#"
            ---
            title: Weekly Menu
            ---

            Menu content here"#},
        )
        .unwrap();

        // Should find file when name includes .menu extension
        let result = get_recipe([&temp_dir_path], &Utf8PathBuf::from("weekly.menu")).unwrap();
        assert_eq!(result.path().as_ref().unwrap(), &menu_path);
    }
}
