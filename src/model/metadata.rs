use serde::{Deserialize, Serialize};
use serde_yaml::Value;
use std::collections::HashMap;

/// Simple metadata structure that holds YAML frontmatter data
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Metadata {
    #[serde(flatten)]
    pub(super) data: HashMap<String, Value>,
}

impl Metadata {
    /// Get the title from metadata
    pub fn title(&self) -> Option<&str> {
        self.data.get("title").and_then(|v| v.as_str())
    }

    /// Get a value from metadata
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.data.get(key)
    }

    /// Get the servings value
    pub fn servings(&self) -> Option<i64> {
        self.data.get("servings").and_then(|v| v.as_i64())
    }

    /// Get image URL from metadata
    /// Checks keys: image, images, picture, pictures
    /// If value is an array, returns the first element
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
            return Ok(Some(yaml_lines.join("\n")));
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
        let yaml_content = "title: Test Recipe\nservings: 4";
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
