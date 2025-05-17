use cooklang::{CooklangParser, Metadata, ScaledRecipe};
use std::sync::OnceLock;
use camino::{Utf8Path, Utf8PathBuf};
use thiserror::Error;

#[derive(Debug)]
pub struct RecipeEntry {
    /// Optional path to the recipe file
    path: Option<Utf8PathBuf>,
    /// Cached string content of the recipe
    content: String,
    /// Cached metadata
    metadata: Metadata,

    /// Cachedd name of the recipe (from file stem or title)
    name: OnceLock<Option<String>>,
    /// Optional path to the title image
    // TODO some data structure for all images instead
    title_image: OnceLock<Option<Utf8PathBuf>>,

    /// Scaling factor for the recipe
    scaling_factor: OnceLock<f64>,

    /// Cached parsed recipe
    // TODO scaled or not?
    recipe: OnceLock<ScaledRecipe>,
}

impl RecipeEntry {
    /// Create a new Recipe instance from a path
    pub fn from_path(path: Utf8PathBuf) -> Result<Self, RecipeEntryError> {
        // Read the file content
        let content = std::fs::read_to_string(&path).map_err(|e| RecipeEntryError::IoError(e))?;

        let (metadata, _warnings) = CooklangParser::canonical()
            .parse_metadata(&content)
            .into_result()
            .unwrap();

        Ok(RecipeEntry {
            path: Some(path.to_path_buf()),
            content: content,
            metadata: metadata,
            name: OnceLock::new(),
            title_image: OnceLock::new(),
            scaling_factor: OnceLock::new(),
            recipe: OnceLock::new(),
        })
    }

    /// Create a new Recipe instance from content
    pub fn from_content(content: String) -> Result<Self, RecipeEntryError> {
        let (metadata, _warnings) = CooklangParser::canonical()
            .parse_metadata(&content)
            .into_result()
            .unwrap();

        Ok(RecipeEntry {
            path: None,
            content: content,
            metadata: metadata,
            name: OnceLock::new(),
            title_image: OnceLock::new(),
            scaling_factor: OnceLock::new(),
            recipe: OnceLock::new(),
        })
    }

    pub fn name(&self) -> &Option<String> {
        self.name.get_or_init(|| {
            if let Some(title) = self.metadata.title() {
                Some(title.to_string())
            } else if let Some(path) = &self.path {
                Some(path.file_stem()?.to_string())
            } else {
                None
            }
        })
    }

    pub fn title_image(&self) -> &Option<Utf8PathBuf> {
        // todo also check metadata
        self.title_image.get_or_init(|| {
            if let Some(path) = &self.path {
                find_title_image(path)
            } else {
                None
            }
        })
    }

    pub fn recipe(&self, scaling_factor: f64) -> &ScaledRecipe {
        self.recipe.get_or_init(|| {
            self.scaling_factor.set(scaling_factor).unwrap();

            let parser = CooklangParser::canonical();

            let (recipe, _warnings) = parser
                .parse(&self.content)
                .into_result()
                .unwrap();

            // Scale the recipe
            recipe.scale(*self.scaling_factor(), parser.converter())
        })
    }

    pub fn scaling_factor(&self) -> &f64 {
        self.scaling_factor.get_or_init(|| 1.0)
    }

    pub fn path(&self) -> &Option<Utf8PathBuf> {
        &self.path
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
        let recipe_path = dir.join(format!("{}.cook", name));
        let mut file = File::create(&recipe_path).unwrap();
        write!(file, "{}", content).unwrap();
        recipe_path
    }

    fn create_test_image(dir: &Utf8Path, name: &str, ext: &str) -> Utf8PathBuf {
        let image_path = dir.join(format!("{}.{}", name, ext));
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
        assert_eq!(recipe.path.as_ref().unwrap(), &recipe_path);
        assert!(recipe.title_image().is_none());
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
        assert_eq!(recipe.title_image().as_ref().unwrap(), &image_path);
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
        assert_eq!(&recipe.content, content);
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

        assert_eq!(metadata.get("servings").unwrap(), 4);
        assert_eq!(metadata.get("time").unwrap(), "30 min");
        assert_eq!(metadata.get("cuisine").unwrap(), "Italian");
    }

    #[test]
    fn test_recipe_parsing() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let content = indoc! {r#"
            ---
            servings: 4
            ---

            Add @salt{1%tsp} and @pepper{1%tsp}"#};
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", content);

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let parsed = recipe.recipe(1.0);

        assert_eq!(parsed.metadata.servings().unwrap()[0], 4);
        assert_eq!(parsed.ingredients.len(), 2);
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

        // Compare paths since we can't implement PartialEq for RecipeEntry
        assert_eq!(recipe1.path, recipe2.path);
        assert_ne!(recipe1.path, recipe3.path);
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

    #[test]
    fn test_recipe_from_content() {
        let content = indoc! {r#"
            ---
            servings: 4
            time: 30 min
            cuisine: Italian
            ---

            Add @salt{1%tsp} and @pepper{1%tsp}"#};

        let recipe = RecipeEntry::from_content(content.to_string()).unwrap();
        assert!(recipe.name().is_none());
        assert!(recipe.path.is_none());
        assert!(recipe.title_image().is_none());

        // Verify content is set
        assert_eq!(&recipe.content, content);

        // Verify metadata is parsed correctly
        let metadata = &recipe.metadata;
        assert_eq!(metadata.get("servings").unwrap(), 4);
        assert_eq!(metadata.get("time").unwrap(), "30 min");
        assert_eq!(metadata.get("cuisine").unwrap(), "Italian");

        // Verify recipe is parsed correctly
        let parsed = recipe.recipe(1.0);
        assert_eq!(parsed.metadata.servings().unwrap()[0], 4);
        assert_eq!(parsed.ingredients.len(), 2);
    }
}
