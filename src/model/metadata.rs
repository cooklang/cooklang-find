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
                if let Some(list) = value_as_list(value) {
                    return list;
                }
            }
        }
        Vec::new()
    }

    /// Returns the metadata value at a dotted key path, such as `"source.url"`.
    ///
    /// Each dot-separated segment is matched case-insensitively: `get_path`
    /// first looks up the leading segment among the top-level frontmatter
    /// keys, then, for every following segment, descends into the previous
    /// value if (and only if) it is a YAML mapping. Returns `None` as soon as
    /// a segment can't be resolved, including when an intermediate value is
    /// not a mapping.
    ///
    /// A key with no dot (e.g. `"title"`) is just a case-insensitive
    /// top-level lookup.
    ///
    /// # Examples
    ///
    /// ```
    /// # use cooklang_find::Metadata;
    /// # let metadata: Metadata = Default::default();
    /// // `source: { name: KoreanBapsang, url: https://... }`
    /// if let Some(url) = metadata.get_path("source.url") {
    ///     println!("source url: {url:?}");
    /// }
    /// ```
    pub fn get_path(&self, dotted_key: &str) -> Option<&Value> {
        let mut segments = dotted_key.split('.');
        let first = segments.next()?;
        let mut current = find_key_case_insensitive(self.data.iter(), first)?;

        for segment in segments {
            let mapping = current.as_mapping()?;
            current = find_key_case_insensitive(
                mapping
                    .iter()
                    .filter_map(|(k, v)| k.as_str().map(|k| (k, v))),
                segment,
            )?;
        }

        Some(current)
    }
}

/// Looks up `key` in `entries` (a stream of `(key, value)` pairs) ignoring
/// ASCII case, returning the first match.
fn find_key_case_insensitive<'a, I, K>(entries: I, key: &str) -> Option<&'a Value>
where
    I: IntoIterator<Item = (K, &'a Value)>,
    K: AsRef<str>,
{
    entries
        .into_iter()
        .find(|(k, _)| k.as_ref().eq_ignore_ascii_case(key))
        .map(|(_, v)| v)
}

/// Interprets a YAML value as a list of strings, the way `Metadata::tags`
/// always has: a comma-separated string is split and trimmed, a sequence
/// keeps its string elements. Returns `None` for any other value shape
/// (including a sequence of non-strings, which yields an empty `Vec` rather
/// than `None` — matching the original `tags()` behavior of still "finding"
/// such a key).
pub(crate) fn value_as_list(value: &Value) -> Option<Vec<String>> {
    if let Some(tag_str) = value.as_str() {
        return Some(
            tag_str
                .split(',')
                .map(|s| s.trim().to_string())
                .filter(|s| !s.is_empty())
                .collect(),
        );
    }
    if let Some(arr) = value.as_sequence() {
        return Some(
            arr.iter()
                .filter_map(|v| v.as_str().map(|s| s.to_string()))
                .collect(),
        );
    }
    None
}

/// Collects every "candidate string" contained in a YAML value: a scalar
/// (string, number, or bool, rendered as text) is its own single candidate;
/// a sequence contributes the candidates of each element; a mapping
/// contributes the candidates of each of its values (keys are not
/// candidates). `null` contributes nothing.
///
/// This is the value shape that `contains`/`equals` conditions search, so
/// that e.g. `source: { name: .., url: https://koreanbapsang.com/x }` is
/// found by a `contains: "koreanbapsang"` condition on `source` just as
/// `source: https://koreanbapsang.com/x` is.
pub(crate) fn value_candidate_strings(value: &Value) -> Vec<String> {
    let mut out = Vec::new();
    collect_value_candidates(value, &mut out);
    out
}

fn collect_value_candidates(value: &Value, out: &mut Vec<String>) {
    match value {
        Value::Null => {}
        Value::Bool(b) => out.push(b.to_string()),
        Value::Number(n) => out.push(n.to_string()),
        Value::String(s) => out.push(s.clone()),
        Value::Sequence(seq) => {
            for v in seq {
                collect_value_candidates(v, out);
            }
        }
        Value::Mapping(map) => {
            for (_, v) in map {
                collect_value_candidates(v, out);
            }
        }
        Value::Tagged(tagged) => collect_value_candidates(&tagged.value, out),
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

    #[test]
    fn get_path_looks_up_a_top_level_key_case_insensitively() {
        let metadata = parse_yaml_content("Cuisine: Japanese").unwrap();
        assert_eq!(
            metadata.get_path("cuisine").unwrap().as_str(),
            Some("Japanese")
        );
        assert_eq!(
            metadata.get_path("CUISINE").unwrap().as_str(),
            Some("Japanese")
        );
    }

    #[test]
    fn get_path_descends_into_a_mapping_case_insensitively() {
        let metadata = parse_yaml_content(
            "source:\n  Name: Korean Bapsang\n  URL: https://koreanbapsang.com/x",
        )
        .unwrap();
        assert_eq!(
            metadata.get_path("source.url").unwrap().as_str(),
            Some("https://koreanbapsang.com/x")
        );
        assert_eq!(
            metadata.get_path("Source.Name").unwrap().as_str(),
            Some("Korean Bapsang")
        );
    }

    #[test]
    fn get_path_returns_none_for_a_missing_key() {
        let metadata = parse_yaml_content("title: Test").unwrap();
        assert!(metadata.get_path("cuisine").is_none());
        assert!(metadata.get_path("missing.nested").is_none());
    }

    #[test]
    fn get_path_returns_none_when_descending_into_a_non_mapping() {
        // `source` is a plain string here, so `source.url` can't resolve.
        let metadata = parse_yaml_content("source: https://koreanbapsang.com/x").unwrap();
        assert!(metadata.get_path("source.url").is_none());
    }

    #[test]
    fn value_as_list_splits_a_comma_separated_string_and_trims() {
        let value = Value::String(" a, b ,c ,, ".to_string());
        assert_eq!(
            value_as_list(&value).unwrap(),
            vec!["a".to_string(), "b".to_string(), "c".to_string()]
        );
    }

    #[test]
    fn value_as_list_keeps_string_elements_of_a_sequence() {
        let metadata = parse_yaml_content("tags: [Korean, easy, 4]").unwrap();
        let value = metadata.get("tags").unwrap();
        assert_eq!(
            value_as_list(value).unwrap(),
            vec!["Korean".to_string(), "easy".to_string()]
        );
    }

    #[test]
    fn value_as_list_is_none_for_a_number_or_bool() {
        assert!(value_as_list(&Value::from(4)).is_none());
        assert!(value_as_list(&Value::from(true)).is_none());
    }

    #[test]
    fn value_candidate_strings_collects_through_maps_and_sequences() {
        let metadata = parse_yaml_content(
            "source:\n  name: Korean Bapsang\n  url: https://koreanbapsang.com/x\ntags: [Korean, easy]\nservings: 4",
        )
        .unwrap();

        let source_candidates = value_candidate_strings(metadata.get("source").unwrap());
        assert!(source_candidates.contains(&"Korean Bapsang".to_string()));
        assert!(source_candidates.contains(&"https://koreanbapsang.com/x".to_string()));

        let tag_candidates = value_candidate_strings(metadata.get("tags").unwrap());
        assert_eq!(
            tag_candidates,
            vec!["Korean".to_string(), "easy".to_string()]
        );

        let servings_candidates = value_candidate_strings(metadata.get("servings").unwrap());
        assert_eq!(servings_candidates, vec!["4".to_string()]);
    }

    #[test]
    fn value_candidate_strings_of_null_is_empty() {
        assert!(value_candidate_strings(&Value::Null).is_empty());
    }
}
