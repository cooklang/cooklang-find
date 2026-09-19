//! Metadata-only filtering: `MetadataFilter` and `Condition`.
//!
//! This is the type a caller builds (or deserializes from JSON) to ask
//! "which recipes have metadata X" without reading recipe bodies. It is
//! deliberately a small, JSON-shaped grammar — the same grammar is mirrored
//! by a TypeScript implementation elsewhere in the Cooklang ecosystem, so the
//! operator names and nesting here must not change casually.

use crate::model::{value_as_list, value_candidate_strings, Metadata, RecipeEntry};
use serde::de::{self, Deserializer};
use serde::ser::{SerializeMap, Serializer};
use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

/// One or many strings, deserialized from either a single JSON string or a
/// JSON array of strings.
///
/// Used for `titleContains`, and for the needle list of a `contains`
/// condition. Serializes back out as an array.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct OneOrMany(pub Vec<String>);

impl OneOrMany {
    /// True if no strings were provided (the field was absent, or an empty
    /// array was given explicitly).
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Iterates over the strings.
    pub fn iter(&self) -> std::slice::Iter<'_, String> {
        self.0.iter()
    }
}

impl<'a> IntoIterator for &'a OneOrMany {
    type Item = &'a String;
    type IntoIter = std::slice::Iter<'a, String>;

    fn into_iter(self) -> Self::IntoIter {
        self.0.iter()
    }
}

impl From<Vec<String>> for OneOrMany {
    fn from(v: Vec<String>) -> Self {
        OneOrMany(v)
    }
}

impl<'de> Deserialize<'de> for OneOrMany {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        #[serde(untagged)]
        enum Repr {
            One(String),
            Many(Vec<String>),
        }

        Ok(match Repr::deserialize(deserializer)? {
            Repr::One(s) => OneOrMany(vec![s]),
            Repr::Many(v) => OneOrMany(v),
        })
    }
}

/// A single condition on the metadata value found at a `MetadataFilter`
/// key.
///
/// Deserializes from a single-key JSON object naming the operator:
///
/// - `{ "contains": "x" }` or `{ "contains": ["x", "y"] }` — case-insensitive
///   substring match against any candidate string of the value (a scalar is
///   its own candidate; a sequence or mapping contributes the candidates of
///   each of its elements/values); true if *any* needle matches *any*
///   candidate.
/// - `{ "equals": "x" }` — case-insensitive whole-string equality against
///   any candidate string of the value.
/// - `{ "has": "x" }` — the value, read as a list the way
///   [`Metadata::tags`](crate::Metadata::tags) already does (an array, or a
///   comma-separated string), contains an element equal to `x`
///   case-insensitively.
/// - `{ "missing": "x" }` — the inverse of `has`; also true when the key is
///   absent entirely.
/// - `{ "exists": true }` / `{ "exists": false }` — whether the key is
///   present at all.
///
/// An object with no recognized key, more than one key, or an unknown key
/// fails to deserialize with a descriptive error.
#[derive(Debug, Clone, PartialEq)]
pub enum Condition {
    /// Case-insensitive substring match; true if any needle matches any
    /// candidate string of the value.
    Contains(OneOrMany),
    /// Case-insensitive whole-string equality against any candidate string.
    Equals(String),
    /// The value (list, or comma-separated string) contains this element,
    /// case-insensitively.
    Has(String),
    /// The inverse of `Has`; also true when the key is absent.
    Missing(String),
    /// Whether the key is present.
    Exists(bool),
}

impl Condition {
    /// Evaluates this condition against the value found at `key` (a dotted
    /// path, see [`Metadata::get_path`]) in `metadata`.
    fn matches(&self, metadata: &Metadata, key: &str) -> bool {
        match self {
            Condition::Exists(want) => metadata.get_path(key).is_some() == *want,
            Condition::Contains(needles) => match metadata.get_path(key) {
                None => false,
                Some(value) => {
                    let candidates = value_candidate_strings(value);
                    needles.iter().any(|needle| {
                        let needle = needle.to_lowercase();
                        candidates
                            .iter()
                            .any(|c| c.to_lowercase().contains(&needle))
                    })
                }
            },
            Condition::Equals(target) => match metadata.get_path(key) {
                None => false,
                Some(value) => {
                    let target = target.to_lowercase();
                    value_candidate_strings(value)
                        .iter()
                        .any(|c| c.to_lowercase() == target)
                }
            },
            Condition::Has(target) => has_element(metadata, key, target),
            Condition::Missing(target) => !has_element(metadata, key, target),
        }
    }
}

/// Shared by `Condition::Has` and `Condition::Missing`: does the value at
/// `key` (read as a list, per [`value_as_list`]) contain `target`
/// case-insensitively? False (not an error) if the key is absent, the value
/// isn't list-shaped, or it's an empty list.
fn has_element(metadata: &Metadata, key: &str, target: &str) -> bool {
    let Some(value) = metadata.get_path(key) else {
        return false;
    };
    let Some(list) = value_as_list(value) else {
        return false;
    };
    list.iter().any(|item| item.eq_ignore_ascii_case(target))
}

impl Serialize for Condition {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        let mut map = serializer.serialize_map(Some(1))?;
        match self {
            Condition::Contains(v) => map.serialize_entry("contains", v)?,
            Condition::Equals(v) => map.serialize_entry("equals", v)?,
            Condition::Has(v) => map.serialize_entry("has", v)?,
            Condition::Missing(v) => map.serialize_entry("missing", v)?,
            Condition::Exists(v) => map.serialize_entry("exists", v)?,
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Condition {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'de>,
    {
        #[derive(Deserialize)]
        struct Raw {
            #[serde(default)]
            contains: Option<OneOrMany>,
            #[serde(default)]
            equals: Option<String>,
            #[serde(default)]
            has: Option<String>,
            #[serde(default)]
            missing: Option<String>,
            #[serde(default)]
            exists: Option<bool>,
            #[serde(flatten)]
            unknown: BTreeMap<String, serde_json::Value>,
        }

        let raw = Raw::deserialize(deserializer)?;

        if !raw.unknown.is_empty() {
            let keys: Vec<_> = raw.unknown.keys().cloned().collect();
            return Err(de::Error::custom(format!(
                "unknown condition operator(s) {}; expected one of \
                 contains, equals, has, missing, exists",
                keys.join(", ")
            )));
        }

        let provided = [
            raw.contains.is_some(),
            raw.equals.is_some(),
            raw.has.is_some(),
            raw.missing.is_some(),
            raw.exists.is_some(),
        ]
        .into_iter()
        .filter(|set| *set)
        .count();

        if provided == 0 {
            return Err(de::Error::custom(
                "empty condition object: expected exactly one of \
                 contains, equals, has, missing, exists",
            ));
        }
        if provided > 1 {
            return Err(de::Error::custom(
                "condition object must have exactly one operator: \
                 contains, equals, has, missing, or exists",
            ));
        }

        Ok(if let Some(v) = raw.contains {
            Condition::Contains(v)
        } else if let Some(v) = raw.equals {
            Condition::Equals(v)
        } else if let Some(v) = raw.has {
            Condition::Has(v)
        } else if let Some(v) = raw.missing {
            Condition::Missing(v)
        } else {
            Condition::Exists(raw.exists.expect("checked above"))
        })
    }
}

/// A metadata-only filter: every condition in `conditions` is ANDed
/// together with `title_contains`, and evaluated against a recipe's
/// frontmatter alone.
///
/// # Examples
///
/// ```
/// use cooklang_find::MetadataFilter;
///
/// let filter = MetadataFilter::from_json(r#"{
///     "where": { "source": { "contains": "koreanbapsang" } },
///     "titleContains": "kimchi"
/// }"#).unwrap();
/// assert!(!filter.is_empty());
/// ```
#[derive(Debug, Clone, Default, PartialEq, Deserialize, Serialize)]
pub struct MetadataFilter {
    /// Frontmatter key (case-insensitive, dotted for nested map values) to
    /// the condition it must satisfy. All entries are ANDed.
    #[serde(default, rename = "where")]
    pub conditions: BTreeMap<String, Condition>,

    /// Case-insensitive substring match against the file stem or the
    /// metadata title; true if the filter's `conditions` are satisfied and
    /// any of these strings is found in either. Empty means "no
    /// constraint".
    #[serde(default, rename = "titleContains")]
    pub title_contains: OneOrMany,
}

impl MetadataFilter {
    /// Parses a `MetadataFilter` from its JSON grammar (see the type-level
    /// docs and [`Condition`]).
    ///
    /// # Examples
    ///
    /// ```
    /// use cooklang_find::MetadataFilter;
    ///
    /// let filter = MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "Japanese"}}}"#)?;
    /// assert!(!filter.is_empty());
    /// # Ok::<(), serde_json::Error>(())
    /// ```
    pub fn from_json(json: &str) -> Result<Self, serde_json::Error> {
        serde_json::from_str(json)
    }

    /// True if this filter has no conditions and no title constraint, so it
    /// matches every recipe.
    pub fn is_empty(&self) -> bool {
        self.conditions.is_empty() && self.title_contains.is_empty()
    }

    /// Evaluates the filter against a recipe entry's metadata (and, for
    /// `title_contains`, its file stem). Reads nothing beyond what `entry`
    /// already has cached — for a path-based entry loaded via
    /// [`RecipeEntry::from_path`], that is the frontmatter only.
    pub fn matches(&self, entry: &RecipeEntry) -> bool {
        let metadata = entry.metadata();

        if !self
            .conditions
            .iter()
            .all(|(key, condition)| condition.matches(metadata, key))
        {
            return false;
        }

        if !self.title_contains.is_empty() {
            let stem = entry
                .path()
                .and_then(|p| p.file_stem())
                .map(|s| s.to_string());
            let title = metadata.title().map(|s| s.to_string());

            let mut candidates = Vec::new();
            if let Some(stem) = &stem {
                candidates.push(stem.as_str());
            }
            if let Some(title) = &title {
                candidates.push(title.as_str());
            }

            let matched = self.title_contains.iter().any(|needle| {
                let needle = needle.to_lowercase();
                candidates
                    .iter()
                    .any(|c| c.to_lowercase().contains(&needle))
            });
            if !matched {
                return false;
            }
        }

        true
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn recipe(content: &str) -> RecipeEntry {
        RecipeEntry::from_content(content.to_string(), None).unwrap()
    }

    fn recipe_named(content: &str, name: &str) -> RecipeEntry {
        RecipeEntry::from_content(content.to_string(), Some(name.to_string())).unwrap()
    }

    // ---- OneOrMany ----

    #[test]
    fn one_or_many_deserializes_a_single_string() {
        let v: OneOrMany = serde_json::from_str(r#""x""#).unwrap();
        assert_eq!(v.0, vec!["x".to_string()]);
    }

    #[test]
    fn one_or_many_deserializes_an_array() {
        let v: OneOrMany = serde_json::from_str(r#"["x", "y"]"#).unwrap();
        assert_eq!(v.0, vec!["x".to_string(), "y".to_string()]);
    }

    // ---- Condition parsing ----

    #[test]
    fn condition_parses_contains_with_one_needle() {
        let c: Condition = serde_json::from_str(r#"{"contains": "x"}"#).unwrap();
        assert_eq!(c, Condition::Contains(OneOrMany(vec!["x".to_string()])));
    }

    #[test]
    fn condition_parses_contains_with_many_needles() {
        let c: Condition = serde_json::from_str(r#"{"contains": ["x", "y"]}"#).unwrap();
        assert_eq!(
            c,
            Condition::Contains(OneOrMany(vec!["x".to_string(), "y".to_string()]))
        );
    }

    #[test]
    fn condition_parses_equals() {
        let c: Condition = serde_json::from_str(r#"{"equals": "Japanese"}"#).unwrap();
        assert_eq!(c, Condition::Equals("Japanese".to_string()));
    }

    #[test]
    fn condition_parses_has() {
        let c: Condition = serde_json::from_str(r#"{"has": "Korean"}"#).unwrap();
        assert_eq!(c, Condition::Has("Korean".to_string()));
    }

    #[test]
    fn condition_parses_missing() {
        let c: Condition = serde_json::from_str(r#"{"missing": "Korean"}"#).unwrap();
        assert_eq!(c, Condition::Missing("Korean".to_string()));
    }

    #[test]
    fn condition_parses_exists_true_and_false() {
        assert_eq!(
            serde_json::from_str::<Condition>(r#"{"exists": true}"#).unwrap(),
            Condition::Exists(true)
        );
        assert_eq!(
            serde_json::from_str::<Condition>(r#"{"exists": false}"#).unwrap(),
            Condition::Exists(false)
        );
    }

    #[test]
    fn condition_rejects_an_unknown_operator() {
        let err = serde_json::from_str::<Condition>(r#"{"startsWith": "x"}"#).unwrap_err();
        assert!(err.to_string().contains("unknown condition operator"));
        assert!(err.to_string().contains("startsWith"));
    }

    #[test]
    fn condition_rejects_an_empty_object() {
        let err = serde_json::from_str::<Condition>(r#"{}"#).unwrap_err();
        assert!(err.to_string().contains("empty condition object"));
    }

    #[test]
    fn condition_rejects_more_than_one_operator() {
        let err =
            serde_json::from_str::<Condition>(r#"{"contains": "x", "equals": "y"}"#).unwrap_err();
        assert!(err.to_string().contains("exactly one operator"));
    }

    // ---- MetadataFilter parsing ----

    #[test]
    fn metadata_filter_from_json_rejects_bad_json() {
        assert!(MetadataFilter::from_json("not json").is_err());
    }

    #[test]
    fn metadata_filter_default_is_empty() {
        assert!(MetadataFilter::default().is_empty());
    }

    #[test]
    fn metadata_filter_from_empty_json_object_is_empty() {
        let filter = MetadataFilter::from_json("{}").unwrap();
        assert!(filter.is_empty());
    }

    // ---- matches(): contains ----

    #[test]
    fn contains_matches_a_scalar_value_case_insensitively() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"source": {"contains": "koreanbapsang"}}}"#)
                .unwrap();
        let entry = recipe("---\nsource: https://KoreanBapsang.com/kimchi\n---\n\nBody");
        assert!(filter.matches(&entry));
    }

    #[test]
    fn contains_matches_a_map_valued_field_through_any_of_its_values() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"source": {"contains": "koreanbapsang"}}}"#)
                .unwrap();
        let entry = recipe(
            "---\nsource:\n  name: Korean Bapsang\n  url: https://koreanbapsang.com/x\n---\n\nBody",
        );
        assert!(filter.matches(&entry));
    }

    #[test]
    fn contains_matches_any_of_several_needles() {
        let filter = MetadataFilter::from_json(
            r#"{"where": {"cuisine": {"contains": ["korean", "thai"]}}}"#,
        )
        .unwrap();
        assert!(filter.matches(&recipe("---\ncuisine: Korean\n---\n\nBody")));
        assert!(!filter.matches(&recipe("---\ncuisine: Japanese\n---\n\nBody")));
    }

    #[test]
    fn contains_is_false_when_the_key_is_absent() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"contains": "korean"}}}"#).unwrap();
        assert!(!filter.matches(&recipe("---\ntitle: Test\n---\n\nBody")));
    }

    // ---- matches(): equals ----

    #[test]
    fn equals_matches_the_whole_string_case_insensitively() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"equals": "japanese"}}}"#).unwrap();
        assert!(filter.matches(&recipe("---\ncuisine: Japanese\n---\n\nBody")));
        assert!(!filter.matches(&recipe("---\ncuisine: Japanese fusion\n---\n\nBody")));
    }

    // ---- matches(): has / missing ----

    #[test]
    fn has_matches_an_array_element_case_insensitively() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"tags": {"has": "korean"}}}"#).unwrap();
        assert!(filter.matches(&recipe("---\ntags: [Korean, easy]\n---\n\nBody")));
        assert!(!filter.matches(&recipe("---\ntags: [Thai, easy]\n---\n\nBody")));
    }

    #[test]
    fn has_matches_a_comma_separated_string_case_insensitively() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"tags": {"has": "korean"}}}"#).unwrap();
        assert!(filter.matches(&recipe("---\ntags: Korean, easy\n---\n\nBody")));
    }

    #[test]
    fn missing_is_true_when_the_key_is_absent() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"tags": {"missing": "korean"}}}"#).unwrap();
        assert!(filter.matches(&recipe("---\ntitle: Test\n---\n\nBody")));
    }

    #[test]
    fn missing_is_the_inverse_of_has_when_the_key_is_present() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"tags": {"missing": "korean"}}}"#).unwrap();
        assert!(!filter.matches(&recipe("---\ntags: [Korean, easy]\n---\n\nBody")));
        assert!(filter.matches(&recipe("---\ntags: [Thai, easy]\n---\n\nBody")));
    }

    // ---- matches(): exists ----

    #[test]
    fn exists_true_requires_the_key() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"exists": true}}}"#).unwrap();
        assert!(filter.matches(&recipe("---\ncuisine: Japanese\n---\n\nBody")));
        assert!(!filter.matches(&recipe("---\ntitle: Test\n---\n\nBody")));
    }

    #[test]
    fn exists_false_requires_the_key_to_be_absent() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"cuisine": {"exists": false}}}"#).unwrap();
        assert!(!filter.matches(&recipe("---\ncuisine: Japanese\n---\n\nBody")));
        assert!(filter.matches(&recipe("---\ntitle: Test\n---\n\nBody")));
    }

    // ---- matches(): dotted keys ----

    #[test]
    fn dotted_key_addresses_a_nested_map_value() {
        let filter = MetadataFilter::from_json(
            r#"{"where": {"source.url": {"contains": "koreanbapsang"}}}"#,
        )
        .unwrap();
        let entry = recipe(
            "---\nsource:\n  name: Korean Bapsang\n  url: https://koreanbapsang.com/x\n---\n\nBody",
        );
        assert!(filter.matches(&entry));

        let non_matching = recipe(
            "---\nsource:\n  name: koreanbapsang\n  url: https://elsewhere.example/x\n---\n\nBody",
        );
        assert!(!filter.matches(&non_matching));
    }

    // ---- matches(): key case-insensitivity ----

    #[test]
    fn condition_key_matches_case_insensitively() {
        let filter =
            MetadataFilter::from_json(r#"{"where": {"CUISINE": {"equals": "Japanese"}}}"#).unwrap();
        assert!(filter.matches(&recipe("---\ncuisine: Japanese\n---\n\nBody")));
    }

    // ---- matches(): ANDing ----

    #[test]
    fn conditions_are_anded() {
        let filter = MetadataFilter::from_json(
            r#"{"where": {"cuisine": {"equals": "Japanese"}, "tags": {"has": "easy"}}}"#,
        )
        .unwrap();
        assert!(filter.matches(&recipe("---\ncuisine: Japanese\ntags: [easy]\n---\n\nBody")));
        assert!(!filter.matches(&recipe("---\ncuisine: Japanese\ntags: [hard]\n---\n\nBody")));
        assert!(!filter.matches(&recipe("---\ncuisine: Thai\ntags: [easy]\n---\n\nBody")));
    }

    // ---- matches(): empty filter ----

    #[test]
    fn empty_filter_matches_everything() {
        let filter = MetadataFilter::default();
        assert!(filter.matches(&recipe("---\ntitle: Test\n---\n\nBody")));
        assert!(filter.matches(&recipe("Body only, no frontmatter")));
    }

    // ---- matches(): titleContains ----

    #[test]
    fn title_contains_matches_the_file_stem_when_there_is_no_title() {
        let temp_dir = tempfile::TempDir::new().unwrap();
        let path =
            camino::Utf8PathBuf::from_path_buf(temp_dir.path().join("Kimchi Stew.cook")).unwrap();
        std::fs::write(&path, "---\nservings: 2\n---\n\nBody").unwrap();

        let filter = MetadataFilter::from_json(r#"{"titleContains": "kimchi"}"#).unwrap();
        let entry = RecipeEntry::from_path(path).unwrap();
        assert!(filter.matches(&entry));
    }

    #[test]
    fn title_contains_does_not_match_a_content_based_entry_with_no_stem_or_title() {
        let filter = MetadataFilter::from_json(r#"{"titleContains": "kimchi"}"#).unwrap();
        let entry = recipe_named("Body only", "Unrelated Name Without Match");
        assert!(!filter.matches(&entry));
    }

    #[test]
    fn title_contains_matches_the_metadata_title_case_insensitively() {
        let filter = MetadataFilter::from_json(r#"{"titleContains": "KIMCHI"}"#).unwrap();
        let entry = recipe("---\ntitle: Spicy Kimchi Stew\n---\n\nBody");
        assert!(filter.matches(&entry));
    }

    #[test]
    fn title_contains_matches_any_of_several_needles() {
        let filter =
            MetadataFilter::from_json(r#"{"titleContains": ["kimchi", "pancake"]}"#).unwrap();
        assert!(filter.matches(&recipe("---\ntitle: Kimchi Stew\n---\n\nBody")));
        assert!(filter.matches(&recipe("---\ntitle: Potato Pancake\n---\n\nBody")));
        assert!(!filter.matches(&recipe("---\ntitle: Waffles\n---\n\nBody")));
    }

    #[test]
    fn title_contains_is_anded_with_conditions() {
        let filter = MetadataFilter::from_json(
            r#"{"where": {"cuisine": {"equals": "Korean"}}, "titleContains": "kimchi"}"#,
        )
        .unwrap();
        assert!(filter.matches(&recipe(
            "---\ntitle: Kimchi Stew\ncuisine: Korean\n---\n\nBody"
        )));
        assert!(!filter.matches(&recipe(
            "---\ntitle: Kimchi Stew\ncuisine: Japanese\n---\n\nBody"
        )));
    }
}
