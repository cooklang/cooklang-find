use super::metadata::{extract_and_parse_metadata, Metadata};
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::fs::File;
use std::io::{BufRead, BufReader};
use std::sync::OnceLock;
use thiserror::Error;

/// The source of a recipe - either from a file path or from content (e.g., stdin)
#[derive(Debug, Serialize, Deserialize)]
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
    // TODO some data structure for all images instead
    #[serde(skip)]
    title_image: OnceLock<Option<String>>,
}

impl RecipeEntry {
    /// Create a new Recipe instance from a path
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
        })
    }

    /// Create a new Recipe instance from content (e.g., stdin)
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
        })
    }

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

    /// Get the raw content of the recipe
    pub fn content(&self) -> Result<String, RecipeEntryError> {
        match &self.source {
            RecipeSource::Path { path } => {
                std::fs::read_to_string(path).map_err(RecipeEntryError::IoError)
            }
            RecipeSource::Content { content, .. } => Ok(content.clone()),
        }
    }

    /// Get the metadata
    pub fn metadata(&self) -> &Metadata {
        &self.metadata
    }

    /// Get the path if this recipe is backed by a file
    pub fn path(&self) -> Option<&Utf8PathBuf> {
        match &self.source {
            RecipeSource::Path { path } => Some(path),
            RecipeSource::Content { .. } => None,
        }
    }
}

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
}
