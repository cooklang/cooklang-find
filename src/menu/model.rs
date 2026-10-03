//! Structured representation of a `.menu` file.
//!
//! A menu is a list of sections (usually days), each holding meals
//! (`Breakfast:`, `Dinner (19:00):`) whose items are recipe references,
//! loose ingredients, connecting text, and notes.

use crate::model::Metadata;
use serde::Serialize;
use std::collections::HashSet;

/// A parsed `.menu` file.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct Menu {
    /// Frontmatter `title`, else the fallback name given to the parser.
    pub name: String,
    /// The menu's YAML frontmatter.
    pub metadata: Metadata,
    /// Sections in file order.
    pub sections: Vec<MenuSection>,
}

/// A `= Header` section of a menu, typically one day.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MenuSection {
    /// Header text with `=` markers trimmed; `None` for content before the
    /// first header.
    pub name: Option<String>,
    /// First `YYYY-MM-DD` in the header, as written.
    pub date: Option<String>,
    /// Meals in file order.
    pub meals: Vec<MenuMeal>,
}

/// A group of items under a `Breakfast:`-style heading.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
pub struct MenuMeal {
    /// Heading without the colon and time, e.g. "Breakfast"; `None` for
    /// items before the first heading in a section.
    pub meal_type: Option<String>,
    /// `HH:MM` from a heading like `Breakfast (08:30):`.
    pub time: Option<String>,
    /// Items in file order.
    pub items: Vec<MenuItem>,
}

/// One entry of a meal.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MenuItem {
    /// A reference to another recipe, e.g. `@./Breakfast/Pancakes{2}`.
    RecipeReference {
        /// Last path component, e.g. "Pancakes".
        name: String,
        /// Path without `./` and `.cook`, e.g. "Breakfast/Pancakes".
        path: String,
        /// Target quantity as authored, e.g. "2" or "1/2".
        quantity: Option<String>,
        /// Target unit as authored, e.g. "servings".
        unit: Option<String>,
        /// Multiplier for the referenced recipe; `None` until
        /// [`Menu::resolve_scales`] runs.
        scale: Option<f64>,
    },
    /// A loose ingredient written directly in the menu.
    Ingredient {
        name: String,
        quantity: Option<String>,
        unit: Option<String>,
    },
    /// Connecting text such as "with". Whitespace runs collapse to one
    /// space; a single leading/trailing space is kept so items can be
    /// concatenated as-is.
    Text { text: String },
    /// A `-- comment`.
    Note { text: String },
}

impl Menu {
    /// Returns every section whose date equals `date`.
    pub fn sections_for_date(&self, date: &str) -> Vec<&MenuSection> {
        self.sections
            .iter()
            .filter(|section| section.date.as_deref() == Some(date))
            .collect()
    }

    /// Returns the distinct section dates in file order.
    pub fn dates(&self) -> Vec<&str> {
        let mut seen = HashSet::new();
        self.sections
            .iter()
            .filter_map(|section| section.date.as_deref())
            .filter(|date| seen.insert(*date))
            .collect()
    }

    /// Returns the earliest and latest section dates.
    ///
    /// Dates are compared as strings, which orders `YYYY-MM-DD` correctly.
    pub fn date_range(&self) -> Option<(&str, &str)> {
        let dates = self.dates();
        let first = dates.iter().min()?;
        let last = dates.iter().max()?;
        Some((*first, *last))
    }

    /// Returns recipe references, deduplicated by path, in first-seen order.
    pub fn recipe_references(&self) -> Vec<&MenuItem> {
        let mut seen = HashSet::new();
        let mut references = Vec::new();
        for item in self.items() {
            if let MenuItem::RecipeReference { path, .. } = item {
                if seen.insert(path.as_str()) {
                    references.push(item);
                }
            }
        }
        references
    }

    fn items(&self) -> impl Iterator<Item = &MenuItem> {
        self.sections
            .iter()
            .flat_map(|section| &section.meals)
            .flat_map(|meal| &meal.items)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn reference(path: &str) -> MenuItem {
        MenuItem::RecipeReference {
            name: path.rsplit('/').next().unwrap().to_string(),
            path: path.to_string(),
            quantity: None,
            unit: None,
            scale: None,
        }
    }

    fn section(name: &str, date: Option<&str>, items: Vec<MenuItem>) -> MenuSection {
        MenuSection {
            name: Some(name.to_string()),
            date: date.map(str::to_string),
            meals: vec![MenuMeal {
                meal_type: Some("Dinner".to_string()),
                time: None,
                items,
            }],
        }
    }

    fn menu(sections: Vec<MenuSection>) -> Menu {
        Menu {
            name: "Plan".to_string(),
            metadata: Metadata::default(),
            sections,
        }
    }

    #[test]
    fn sections_for_date_returns_all_matching_sections() {
        let m = menu(vec![
            section("2026-06-24 Lunch", Some("2026-06-24"), vec![]),
            section("2026-06-25", Some("2026-06-25"), vec![]),
            section("2026-06-24 Dinner", Some("2026-06-24"), vec![]),
        ]);

        let names: Vec<_> = m
            .sections_for_date("2026-06-24")
            .iter()
            .map(|s| s.name.as_deref().unwrap())
            .collect();

        assert_eq!(names, vec!["2026-06-24 Lunch", "2026-06-24 Dinner"]);
    }

    #[test]
    fn dates_are_distinct_in_file_order() {
        let m = menu(vec![
            section("b", Some("2026-03-08"), vec![]),
            section("a", Some("2026-03-07"), vec![]),
            section("b again", Some("2026-03-08"), vec![]),
            section("Day 4", None, vec![]),
        ]);

        assert_eq!(m.dates(), vec!["2026-03-08", "2026-03-07"]);
    }

    #[test]
    fn date_range_spans_min_to_max() {
        let m = menu(vec![
            section("b", Some("2026-03-08"), vec![]),
            section("a", Some("2026-03-07"), vec![]),
            section("c", Some("2026-03-09"), vec![]),
        ]);

        assert_eq!(m.date_range(), Some(("2026-03-07", "2026-03-09")));
    }

    #[test]
    fn date_range_is_none_without_dates() {
        let m = menu(vec![section("Day 1", None, vec![])]);

        assert_eq!(m.date_range(), None);
    }

    #[test]
    fn recipe_references_dedup_by_path() {
        let m = menu(vec![
            section("1", None, vec![reference("Risotto"), reference("Soup")]),
            section(
                "2",
                None,
                vec![
                    MenuItem::Ingredient {
                        name: "milk".to_string(),
                        quantity: None,
                        unit: None,
                    },
                    reference("Risotto"),
                ],
            ),
        ]);

        let refs = m.recipe_references();

        assert_eq!(refs, vec![&reference("Risotto"), &reference("Soup")]);
    }

    #[test]
    fn items_serialize_with_kind_tag() {
        let json = serde_json::to_value(MenuItem::Text {
            text: "with".to_string(),
        })
        .unwrap();

        assert_eq!(json, serde_json::json!({"kind": "text", "text": "with"}));
    }
}
