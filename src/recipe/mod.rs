use cooklang::{
    quantity::ScalableValue, scale::Servings, Converter, CooklangParser, Extensions,
    Recipe as CooklangRecipe,
};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use thiserror::Error;

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

#[derive(Debug, Serialize, Deserialize)]
pub struct Recipe {
    /// Name of the recipe (file stem)
    pub name: String,
    /// Path to the recipe file
    pub path: PathBuf,
    /// Optional path to the title image
    pub title_image: Option<PathBuf>,
    /// Cached content of the recipe file
    #[serde(skip)]
    content: Option<String>,
    /// Cached parsed recipe
    #[serde(skip)]
    parsed: Option<CooklangRecipe<Servings, ScalableValue>>,
    /// Cached metadata
    #[serde(skip)]
    metadata: Option<HashMap<String, String>>,
}

impl Clone for Recipe {
    fn clone(&self) -> Self {
        Recipe {
            name: self.name.clone(),
            path: self.path.clone(),
            title_image: self.title_image.clone(),
            content: self.content.clone(),
            parsed: None, // Don't clone the parsed recipe, it can be re-parsed if needed
            metadata: None,
        }
    }
}

impl PartialEq for Recipe {
    fn eq(&self, other: &Self) -> bool {
        self.path == other.path
    }
}

impl Eq for Recipe {}

impl Hash for Recipe {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.path.hash(state);
    }
}

impl Recipe {
    /// Create a new Recipe instance from a path
    pub(crate) fn new(path: PathBuf) -> Result<Self, RecipeError> {
        let name = path
            .file_stem()
            .ok_or_else(|| RecipeError::InvalidPath(path.clone()))?
            .to_string_lossy()
            .into_owned();

        let title_image = find_title_image(&path);

        Ok(Recipe {
            name,
            path,
            title_image,
            content: None,
            parsed: None,
            metadata: None,
        })
    }

    /// Get the content of the recipe file
    pub fn content(&mut self) -> Result<&str, RecipeError> {
        if self.content.is_none() {
            let content = fs::read_to_string(&self.path)?;
            self.content = Some(content);
        }
        Ok(self.content.as_ref().unwrap())
    }

    /// Parse the recipe and return the parsed representation
    pub fn recipe(&mut self) -> Result<&CooklangRecipe<Servings, ScalableValue>, RecipeError> {
        if self.parsed.is_none() {
            let content = self.content()?;
            let parser = CooklangParser::new(Extensions::default(), Converter::default());
            let pass_result = parser.parse(content);
            match pass_result.into_result() {
                Ok((recipe, _warnings)) => {
                    self.parsed = Some(recipe);
                }
                Err(e) => {
                    return Err(RecipeError::ParseError(e.to_string()));
                }
            }
        }
        Ok(self.parsed.as_ref().unwrap())
    }

    /// Parse only the metadata of the recipe
    pub fn metadata(&mut self) -> Result<&HashMap<String, String>, RecipeError> {
        if self.metadata.is_none() {
            let content = self.content()?;
            let parser = CooklangParser::new(Extensions::default(), Converter::default());
            let pass_result = parser.parse_metadata(content);
            match pass_result.into_result() {
                Ok((metadata, _warnings)) => {
                    let metadata_map: HashMap<String, String> = metadata
                        .map
                        .into_iter()
                        .map(|(k, v)| {
                            let value = if let Some(s) = v.as_str() {
                                s.to_string()
                            } else if let Some(i) = v.as_i64() {
                                i.to_string()
                            } else if let Some(f) = v.as_f64() {
                                f.to_string()
                            } else {
                                v.as_str().unwrap_or_default().to_string()
                            };
                            (k.as_str().unwrap_or_default().to_string(), value)
                        })
                        .collect();
                    self.metadata = Some(metadata_map);
                }
                Err(e) => {
                    return Err(RecipeError::MetadataError(e.to_string()));
                }
            }
        }
        Ok(self.metadata.as_ref().unwrap())
    }

    /// Get the path to the title image if it exists
    pub fn title_image(&self) -> Option<&Path> {
        self.title_image.as_deref()
    }
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

        let recipe = Recipe::new(recipe_path.clone()).unwrap();
        assert_eq!(recipe.name, "test_recipe");
        assert_eq!(recipe.path, recipe_path);
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

        let recipe = Recipe::new(recipe_path).unwrap();
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

        let mut recipe = Recipe::new(recipe_path).unwrap();
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

        let mut recipe = Recipe::new(recipe_path).unwrap();
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

        let mut recipe = Recipe::new(recipe_path).unwrap();
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

        let mut original = Recipe::new(recipe_path).unwrap();
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

        let recipe1 = Recipe::new(path1.clone()).unwrap();
        let recipe2 = Recipe::new(path1).unwrap();
        let recipe3 = Recipe::new(path2).unwrap();

        assert_eq!(recipe1, recipe2);
        assert_ne!(recipe1, recipe3);
    }

    #[test]
    fn test_invalid_recipe_path() {
        let temp_dir = TempDir::new().unwrap();
        let invalid_path = temp_dir.path().join("nonexistent.cook");

        let mut recipe = Recipe::new(invalid_path).unwrap();
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
}
