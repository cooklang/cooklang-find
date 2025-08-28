use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;

/// Represents metadata extracted from recipe/menu YAML frontmatter.
///
/// This structure provides convenient access to common metadata fields
/// like title, servings, tags, and images, while also allowing access
/// to any custom metadata fields through the `get()` method.
///
/// # Examples
///
/// ```no_run
/// # use cooklang_find::Metadata;
/// # let metadata: Metadata = Default::default();
/// // Access common metadata fields
/// let title = metadata.title();
/// let servings = metadata.servings();
/// let tags = metadata.tags();
/// let image_url = metadata.image_url();
///
/// // Access custom fields
/// if let Some(cuisine) = metadata.get("cuisine") {
///     println!("Cuisine: {:?}", cuisine);
/// }
/// ```
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Metadata {
    #[serde(flatten)]
    pub(super) data: HashMap<String, Value>,
}

impl Metadata {
    /// Returns the recipe title from metadata.
    ///
    /// Returns `None` if no title field is present in the metadata.
    pub fn title(&self) -> Option<&str> {
        self.data.get("title").and_then(|v| v.as_str())
    }

    /// Returns a metadata value by key.
    ///
    /// This method provides access to any metadata field, including
    /// custom fields not covered by the convenience methods.
    ///
    /// # Arguments
    ///
    /// * `key` - The metadata field name to retrieve
    ///
    /// # Returns
    ///
    /// Returns the YAML value if the key exists, or `None` otherwise.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    /// Returns the number of servings from metadata.
    ///
    /// Returns `None` if no servings field is present or if it's not a number.
    pub fn servings(&self) -> Option<i64> {
        self.data.get("servings").and_then(|v| v.as_i64())
    }

    /// Returns the primary image URL from metadata.
    ///
    /// Searches for image URLs in the following metadata keys (in order):
    /// - `image` (string)
    /// - `images` (array - returns first element)
    /// - `picture` (string)
    /// - `pictures` (array - returns first element)
    ///
    /// # Returns
    ///
    /// Returns the first image URL found, or `None` if no image fields exist.
    pub fn image_url(&self) -> Option<String> {
        const IMAGE_KEYS: &[&str] = &["image", "images", "picture", "pictures"];

        for key in IMAGE_KEYS {
            if let Some(value) = self.data.get(*key) {
                // If it's a string, return it
                if let Some(url) = value.as_str() {
                    return Some(url.to_string());
                }
                // If it's an array, return the first element
                if let Some(arr) = value.as_sequence() {
                    if let Some(first) = arr.first() {
                        if let Some(url) = first.as_str() {
                            return Some(url.to_string());
                        }
                    }
                }
            }
        }
        None
    }

    /// Returns all tags from metadata.
    ///
    /// Searches for tags in the following metadata keys (in order):
    /// - `tags` (comma-separated string or array)
    /// - `tag` (comma-separated string or array)
    ///
    /// # Returns
    ///
    /// Returns a vector of tag strings. Returns an empty vector if no tags are found.
    pub fn tags(&self) -> Vec<String> {
        const TAG_KEYS: &[&str] = &["tags", "tag"];

        for key in TAG_KEYS {
            if let Some(value) = self.data.get(*key) {
                // If it's a string, split by comma and return
                if let Some(tag_str) = value.as_str() {
                    return tag_str
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .filter(|s| !s.is_empty())
                        .collect();
                }
                // If it's an array, return all string elements
                if let Some(arr) = value.as_sequence() {
                    return arr
                        .iter()
                        .filter_map(|v| v.as_str().map(|s| s.to_string()))
                        .collect();
                }
            }
        }
        Vec::new()
    }
}

/// Parse YAML frontmatter from raw YAML content (without --- markers)
/// Returns None if the content is empty or invalid YAML
pub(super) fn parse_yaml_content(yaml_content: &str) -> Option<Metadata> {
    if yaml_content.trim().is_empty() {
        return None;
    }

    serde_yaml::from_str::<HashMap<String, Value>>(yaml_content)
        .ok()
        .map(|data| Metadata { data })
}

/// Extract YAML from a Result iterator (for file reading)
pub(super) fn extract_yaml_from_lines<I, E>(mut lines: I) -> Result<Option<String>, E>
where
    I: Iterator<Item = Result<String, E>>,
{
    // Check first line
    let first_line = match lines.next() {
        Some(Ok(line)) => line,
        Some(Err(e)) => return Err(e),
        None => return Ok(None), // Empty file
    };

    if !first_line.trim().eq("---") {
        return Ok(None); // No frontmatter
    }

    // Collect YAML lines until closing ---
    let mut yaml_lines = Vec::new();
    for line_result in lines {
        let line = line_result?;
        if line.trim().eq("---") {
            // Found closing marker, return the YAML content
            return Ok(Some(yaml_lines.join(
                "
",
            )));
        }
        yaml_lines.push(line);
        // Prevent reading too many lines
        if yaml_lines.len() > 30 {
            return Ok(None);
        }
    }

    // No closing marker
    Ok(None)
}

/// Helper to extract and parse metadata from a Result iterator
pub(super) fn extract_and_parse_metadata<I, E>(lines: I) -> Result<Metadata, E>
where
    I: Iterator<Item = Result<String, E>>,
{
    let yaml_content = extract_yaml_from_lines(lines)?;
    Ok(yaml_content
        .and_then(|content| parse_yaml_content(&content))
        .unwrap_or_default())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_yaml_content() {
        // Test valid YAML
        let yaml_content = "title: Test Recipe
servings: 4";
        let metadata = parse_yaml_content(yaml_content);
        assert!(metadata.is_some());
        let metadata = metadata.unwrap();
        assert_eq!(metadata.title().unwrap(), "Test Recipe");
        assert_eq!(metadata.servings().unwrap(), 4);

        // Test invalid YAML
        let yaml_content = "invalid: yaml: content:";
        let metadata = parse_yaml_content(yaml_content);
        assert!(metadata.is_none());

        // Test empty content
        let yaml_content = "";
        let metadata = parse_yaml_content(yaml_content);
        assert!(metadata.is_none());
    }
}
