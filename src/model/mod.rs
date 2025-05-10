use cooklang::{
    quantity::ScalableValue, scale::Servings, Converter, CooklangParser, Extensions,
    Recipe as CooklangRecipe,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use once_cell::sync::OnceCell;
use thiserror::Error;


#[derive(Debug, Serialize, Deserialize)]
pub struct RecipeEntry {
    /// Cachedd name of the recipe (from file stem or title)
    name: Option<String>,
    /// Optional path to the recipe file
    path: Option<PathBuf>,
    /// Optional path to the title image
    title_image: Option<PathBuf>,
    /// Cached string content of the recipe
    #[serde(skip)]
    content: Option<String>,
    /// Cached parsed recipe
    #[serde(skip)]
    parsed: Option<CooklangRecipe<Servings, ScalableValue>>,
    /// Cached metadata
    #[serde(skip)]
    metadata: Option<HashMap<String, String>>,
}


impl RecipeEntry {
    /// Create a new Recipe instance from a path
    pub fn from_path(path: PathBuf) -> Result<Self, RecipeError> {
        let title_image = find_title_image(&path);

        Ok(RecipeEntry {
            name: None,
            path: Some(path),
            title_image,
            content: None,
            parsed: None,
            metadata: None,
        })
    }

    /// Create a new Recipe instance from content
    pub fn from_content(content: String) -> Result<Self, RecipeError> {
        Ok(RecipeEntry {
            name: None,
            path: None,
            title_image: None,
            content: Some(content),
            parsed: None,
            metadata: None,
        })
    }
}

#[derive(Error, Debug)]
pub enum RecipeError {
    #[error("Failed to read recipe file: {0}")]
    IoError(#[from] std::io::Error),

    #[error("Failed to get file stem from path: {0}")]
    InvalidPath(PathBuf),

    #[error("Failed to parse recipe: {0}")]
    ParseError(String),

    #[error("Failed to parse recipe metadata: {0}")]
    MetadataError(String),
}

fn find_title_image(path: &Path) -> Option<PathBuf> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use indoc::indoc;
    use std::fs::File;
    use std::io::Write;
    use tempfile::TempDir;

    fn create_test_recipe(dir: &Path, name: &str, content: &str) -> PathBuf {
        let recipe_path = dir.join(format!("{}.cook", name));
        let mut file = File::create(&recipe_path).unwrap();
        write!(file, "{}", content).unwrap();
        recipe_path
    }

    fn create_test_image(dir: &Path, name: &str, ext: &str) -> PathBuf {
        let image_path = dir.join(format!("{}.{}", name, ext));
        File::create(&image_path).unwrap();
        image_path
    }

    #[test]
    fn test_recipe_creation() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(
            temp_dir.path(),
            "test_recipe",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );

        let recipe = RecipeEntry::from_path(recipe_path.clone()).unwrap();
        assert_eq!(recipe.name.as_ref().unwrap(), "test_recipe");
        assert_eq!(recipe.path.as_ref().unwrap(), &recipe_path);
        assert!(recipe.title_image.is_none());
    }

    #[test]
    fn test_recipe_with_title_image() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(
            temp_dir.path(),
            "test_recipe",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );
        let image_path = create_test_image(temp_dir.path(), "test_recipe", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        assert_eq!(recipe.title_image.as_ref().unwrap(), &image_path);
    }

    #[test]
    fn test_recipe_content() {
        let temp_dir = TempDir::new().unwrap();
        let content = indoc! {r#"
            ---
            servings: 4
            ---

            Test recipe content"#};
        let recipe_path = create_test_recipe(temp_dir.path(), "test_recipe", content);

        let mut recipe = RecipeEntry::from_path(recipe_path).unwrap();
        assert_eq!(recipe.content().unwrap(), content);
    }

    #[test]
    fn test_recipe_metadata() {
        let temp_dir = TempDir::new().unwrap();
        let content = indoc! {r#"
            ---
            servings: 4
            time: 30 min
            cuisine: Italian
            ---

            Test recipe content"#};
        let recipe_path = create_test_recipe(temp_dir.path(), "test_recipe", content);

        let mut recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let metadata = recipe.metadata().unwrap();

        assert_eq!(metadata.get("servings").unwrap(), "4");
        assert_eq!(metadata.get("time").unwrap(), "30 min");
        assert_eq!(metadata.get("cuisine").unwrap(), "Italian");
    }

    #[test]
    fn test_recipe_parsing() {
        let temp_dir = TempDir::new().unwrap();
        let content = indoc! {r#"
            ---
            servings: 4
            ---

            Add @salt{1%tsp} and @pepper{1%tsp}"#};
        let recipe_path = create_test_recipe(temp_dir.path(), "test_recipe", content);

        let mut recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let parsed = recipe.recipe().unwrap();

        assert_eq!(parsed.metadata.servings().unwrap()[0], 4);
        assert_eq!(parsed.ingredients.len(), 2);
    }

    #[test]
    fn test_recipe_clone() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(
            temp_dir.path(),
            "test_recipe",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );

        let mut original = RecipeEntry::from_path(recipe_path).unwrap();
        original.content().unwrap(); // Load content

        let cloned = original.clone();
        assert_eq!(cloned.name, original.name);
        assert_eq!(cloned.path, original.path);
        assert_eq!(cloned.content, original.content);
        assert!(cloned.parsed.is_none()); // Parsed content should not be cloned
    }

    #[test]
    fn test_recipe_equality() {
        let temp_dir = TempDir::new().unwrap();
        let path1 = create_test_recipe(
            temp_dir.path(),
            "recipe1",
            indoc! {r#"
                ---
                servings: 4
                ---

                Test recipe content"#},
        );
        let path2 = create_test_recipe(
            temp_dir.path(),
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

        assert_eq!(recipe1, recipe2);
        assert_ne!(recipe1, recipe3);
    }

    #[test]
    fn test_invalid_recipe_path() {
        let temp_dir = TempDir::new().unwrap();
        let invalid_path = temp_dir.path().join("nonexistent.cook");

        let mut recipe = RecipeEntry::from_path(invalid_path).unwrap();
        assert!(recipe.content().is_err());
    }

    #[test]
    fn test_find_title_image_no_image() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(temp_dir.path(), "test_recipe", "Test content");
        assert!(find_title_image(&recipe_path).is_none());
    }

    #[test]
    fn test_find_title_image_all_extensions() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(temp_dir.path(), "test_recipe", "Test content");

        // Test each supported extension
        for ext in ["jpg", "jpeg", "png", "webp"] {
            // Clean up any previous test images
            for old_ext in ["jpg", "jpeg", "png", "webp"] {
                let _ = std::fs::remove_file(recipe_path.with_extension(old_ext));
            }

            let image_path = create_test_image(temp_dir.path(), "test_recipe", ext);
            let found = find_title_image(&recipe_path);

            assert!(
                found.is_some(),
                "Failed to find image with extension {}",
                ext
            );
            assert_eq!(found.unwrap(), image_path);
        }
    }

    #[test]
    fn test_find_title_image_multiple_images() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(temp_dir.path(), "test_recipe", "Test content");

        // Create images with different extensions
        let jpg_path = create_test_image(temp_dir.path(), "test_recipe", "jpg");
        let _png_path = create_test_image(temp_dir.path(), "test_recipe", "png");
        let _webp_path = create_test_image(temp_dir.path(), "test_recipe", "webp");

        // Should return the first matching extension (jpg)
        let found_image = find_title_image(&recipe_path);
        assert!(found_image.is_some());
        assert_eq!(found_image.unwrap(), jpg_path);
    }

    #[test]
    fn test_find_title_image_case_sensitivity() {
        let temp_dir = TempDir::new().unwrap();
        let recipe_path = create_test_recipe(temp_dir.path(), "test_recipe", "Test content");

        // Create an image with uppercase extension
        let image_path = temp_dir.path().join("test_recipe.JPG");
        File::create(&image_path).unwrap();
        let found_image = find_title_image(&recipe_path);

        // Should find the image with uppercase extension
        assert!(found_image.is_some());
    }

    #[test]
    fn test_recipe_from_content() {
        let content = indoc! {r#"
            ---
            servings: 4
            time: 30 min
            cuisine: Italian
            ---

            Add @salt{1%tsp} and @pepper{1%tsp}"#};

        let mut recipe = RecipeEntry::from_content(content.to_string()).unwrap();
        assert!(recipe.name.is_none());
        assert!(recipe.path.is_none());
        assert!(recipe.title_image.is_none());

        // Verify content is set
        assert_eq!(recipe.content().unwrap(), content);

        // Verify metadata is parsed correctly
        let metadata = recipe.metadata().unwrap();
        assert_eq!(metadata.get("servings").unwrap(), "4");
        assert_eq!(metadata.get("time").unwrap(), "30 min");
        assert_eq!(metadata.get("cuisine").unwrap(), "Italian");

        // Verify recipe is parsed correctly
        let parsed = recipe.recipe().unwrap();
        assert_eq!(parsed.metadata.servings().unwrap()[0], 4);
        assert_eq!(parsed.ingredients.len(), 2);
    }
}
