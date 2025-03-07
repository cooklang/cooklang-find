use std::path::{Path, PathBuf};
use std::fs;
use anyhow::{Result, Context};
use cooklang::{
    CooklangParser, Recipe as CooklangRecipe, Extensions, Converter,
    scale::Servings,
    quantity::ScalableValue,
};
use serde::{Serialize, Deserialize};
use glob::glob;
use std::collections::HashSet;
use std::hash::{Hash, Hasher};

// TODO, reuse parser

#[derive(Debug, Serialize, Deserialize)]
pub struct Recipe {
    /// Name of the recipe (file stem)
    pub name: String,
    /// Path to the recipe file
    path: PathBuf,
    /// Optional path to the title image
    title_image: Option<PathBuf>,
    /// Cached content of the recipe file
    #[serde(skip)]
    content: Option<String>,
    /// Cached parsed recipe
    #[serde(skip)]
    parsed: Option<CooklangRecipe<Servings, ScalableValue>>,
}

impl Clone for Recipe {
    fn clone(&self) -> Self {
        Recipe {
            name: self.name.clone(),
            path: self.path.clone(),
            title_image: self.title_image.clone(),
            content: self.content.clone(),
            parsed: None, // Don't clone the parsed recipe, it can be re-parsed if needed
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
    fn new(path: PathBuf) -> Result<Self> {
        let name = path.file_stem()
            .context("Failed to get file stem")?
            .to_string_lossy()
            .into_owned();

        // Look for an image with the same stem
        let possible_image_extensions = ["jpg", "jpeg", "png", "webp"];
        let title_image = possible_image_extensions.iter()
            .find_map(|ext| {
                let image_path = path.with_extension(ext);
                if image_path.exists() {
                    Some(image_path)
                } else {
                    None
                }
            });

        Ok(Recipe {
            name,
            path,
            title_image,
            content: None,
            parsed: None,
        })
    }

    /// Get the content of the recipe file
    pub fn content(&mut self) -> Result<&str> {
        if self.content.is_none() {
            let content = fs::read_to_string(&self.path)
                .with_context(|| format!("Failed to read recipe file: {}", self.path.display()))?;
            self.content = Some(content);
        }
        Ok(self.content.as_ref().unwrap())
    }

    /// Parse the recipe and return the parsed representation
    pub fn parse(&mut self) -> Result<&CooklangRecipe<Servings, ScalableValue>> {
        if self.parsed.is_none() {
            let content = self.content()?;
            let parser = CooklangParser::new(Extensions::default(), Converter::default());
            let pass_result = parser.parse(content);
            match pass_result.into_result() {
                Ok((recipe, _warnings)) => {
                    self.parsed = Some(recipe);
                }
                Err(e) => {
                    return Err(anyhow::anyhow!("Failed to parse recipe {}: {}", self.path.display(), e));
                }
            }
        }
        Ok(self.parsed.as_ref().unwrap())
    }

    /// Get the path to the title image if it exists
    pub fn title_image(&self) -> Option<&Path> {
        self.title_image.as_deref()
    }
}

/// Recipe finder configuration
#[derive(Debug, Clone)]
pub struct RecipeFinder {
    base_dirs: Vec<PathBuf>,
}

impl RecipeFinder {
    /// Create a new RecipeFinder with the given base directories
    pub fn new<P: AsRef<Path>>(base_dirs: Vec<P>) -> Self {
        RecipeFinder {
            base_dirs: base_dirs.into_iter().map(|p| p.as_ref().to_owned()).collect(),
        }
    }

    /// Find a recipe by name
    pub fn get<P: AsRef<Path>>(&self, name: P) -> Result<Option<Recipe>> {
        let name = name.as_ref();

        // If the path is absolute, try to load it directly
        if name.is_absolute() {
            if name.exists() {
                return Ok(Some(Recipe::new(name.to_owned())?));
            }
            return Ok(None);
        }

        // Try each base directory
        for base_dir in &self.base_dirs {
            // Try with and without .cook extension
            let with_ext = base_dir.join(name).with_extension("cook");
            if with_ext.exists() {
                return Ok(Some(Recipe::new(with_ext)?));
            }

            let as_is = base_dir.join(name);
            if as_is.exists() && as_is.extension().map_or(false, |ext| ext == "cook") {
                return Ok(Some(Recipe::new(as_is)?));
            }
        }

        Ok(None)
    }

    /// Search for recipes containing the given text
    pub fn search(&self, query: &str) -> Result<Vec<Recipe>> {
        let mut results = HashSet::new();

        for base_dir in &self.base_dirs {
            let pattern = base_dir.join("**/*.cook");
            let pattern = pattern.to_string_lossy();

            for entry in glob(&pattern)? {
                let path = entry?;
                let content = fs::read_to_string(&path)?;
                if content.to_lowercase().contains(&query.to_lowercase()) {
                    results.insert(Recipe::new(path)?);
                }
            }
        }

        Ok(results.into_iter().collect())
    }

    /// List all recipes in the base directories
    pub fn list_all(&self) -> Result<Vec<Recipe>> {
        let mut results = HashSet::new();

        for base_dir in &self.base_dirs {
            let pattern = base_dir.join("**/*.cook");
            let pattern = pattern.to_string_lossy();

            for entry in glob(&pattern)? {
                let path = entry?;
                results.insert(Recipe::new(path)?);
            }
        }

        Ok(results.into_iter().collect())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs::File;
    use std::io::Write;
    use tempfile::tempdir;

    #[test]
    fn test_recipe_finder() -> Result<()> {
        let temp_dir = tempdir()?;
        let recipe_path = temp_dir.path().join("pancakes.cook");
        let mut file = File::create(&recipe_path)?;
        writeln!(file, ">> servings: 4\n\nMix @flour{{200 g}} with @milk{{300 ml}}")?;

        let finder = RecipeFinder::new(vec![temp_dir.path()]);

        // Test get by name
        let mut recipe = finder.get("pancakes")?.expect("Recipe should exist");
        assert_eq!(recipe.name, "pancakes");

        // Test content reading
        let content = recipe.content()?;
        assert!(content.contains("flour"));

        // Test parsing
        let parsed = recipe.parse()?;
        assert_eq!(parsed.metadata.map.get("servings").unwrap().as_str().unwrap(), "4");

        Ok(())
    }
}