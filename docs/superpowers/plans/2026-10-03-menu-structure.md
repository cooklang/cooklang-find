# Menu Structure Implementation Plan

> **Note:** The spec (`docs/superpowers/specs/2026-10-03-menu-structure-design.md`) is authoritative; this plan's code snippets predate the review fixes.

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Parse `.menu` files into a structured `Menu` (sections → meals → items) with date/recipe helpers, reference scale resolution, and uniffi bindings.

**Architecture:** Three new files under `src/menu/`: `model.rs` (types + helpers), `parse.rs` (line scanner, no `cooklang` dependency), `scale.rs` (resolves `{target%unit}` on recipe references using referenced recipes' `servings`/`yield`). `RecipeEntry::menu()` and FFI records wrap it. Shape mirrors CookCLI's `GET /api/menus/*path`.

**Tech Stack:** Rust, `regex`, `serde`, `serde_yaml`, `camino`, `uniffi`; tests use `indoc`, `tempfile`.

---

## Spec

See `docs/superpowers/specs/2026-10-03-menu-structure-design.md`.

## File Structure

- **Create:** `src/menu/model.rs` — `Menu`, `MenuSection`, `MenuMeal`, `MenuItem`, helper methods.
- **Create:** `src/menu/parse.rs` — `Menu::parse` and private scanning helpers.
- **Create:** `src/menu/scale.rs` — `Menu::resolve_scales` and number/yield parsing.
- **Modify:** `src/menu/mod.rs` — declare submodules, re-export types, update module doc.
- **Modify:** `src/model/metadata.rs` — derive `PartialEq` on `Metadata`; make `parse_yaml_content` `pub(crate)`.
- **Modify:** `src/model/mod.rs` — re-export `parse_yaml_content` crate-wide.
- **Modify:** `src/model/recipe_entry.rs` — add `RecipeEntry::menu()`.
- **Modify:** `src/lib.rs` — re-export the new types.
- **Modify:** `src/ffi.rs` — `FfiMenu*` types, `parse_menu`, `parse_menu_content`.
- **Modify:** `BINDINGS.md` — document the new API.

Commands: `cargo test --lib menu::` for menu tests; `make lint` runs `cargo fmt --check` + `cargo clippy -- -D warnings`.

---

## Task 1: Model types and helpers

**Files:**
- Create: `src/menu/model.rs`
- Modify: `src/menu/mod.rs`, `src/model/metadata.rs:27`

- [ ] **Step 1: Derive `PartialEq` on `Metadata`**

In `src/model/metadata.rs`, change:

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default)]
pub struct Metadata {
```

to:

```rust
#[derive(Debug, Serialize, Deserialize, Clone, Default, PartialEq)]
pub struct Metadata {
```

- [ ] **Step 2: Write `src/menu/model.rs` with types and failing tests**

```rust
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
```

- [ ] **Step 3: Register the module in `src/menu/mod.rs`**

Replace the module doc and imports at the top of `src/menu/mod.rs` (lines 1-15) with:

```rust
//! Menu files: discovery by date and structured parsing.
//!
//! [`list_menus_for_date`] finds `.menu` files that are "active" on a given
//! date. A menu file is active for a date when one of its section headers
//! contains that date string (for example, a section `= 2026-06-24 Dinner`
//! makes the file active for `2026-06-24`).
//!
//! [`Menu`] is the structured form of a menu: sections, meals, and items.
//!
//! The library performs no date parsing or timezone handling: the caller
//! supplies the date as an opaque string that is matched literally.

mod model;

pub use model::{Menu, MenuItem, MenuMeal, MenuSection};

use crate::model::lossy::read_to_string_lossy;
use crate::model::RecipeEntry;
use camino::{Utf8Path, Utf8PathBuf};
use std::collections::HashSet;
use thiserror::Error;
```

- [ ] **Step 4: Run tests**

Run: `cargo test --lib menu::model`
Expected: 6 tests PASS (the helpers are implemented alongside the types; this step confirms they compile and behave).

- [ ] **Step 5: Commit**

```bash
git add src/menu/model.rs src/menu/mod.rs src/model/metadata.rs
git commit -m "feat(menu): add Menu model and helpers"
```

---

## Task 2: Item scanner

Scans the text of one line into items. Lives in `parse.rs`; Task 3 adds the line-level parser to the same file.

**Files:**
- Create: `src/menu/parse.rs`
- Modify: `src/menu/mod.rs`

- [ ] **Step 1: Write failing tests in `src/menu/parse.rs`**

```rust
//! Line scanner that turns `.menu` content into a [`Menu`].

use super::model::MenuItem;

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &str) -> Vec<MenuItem> {
        let mut items = Vec::new();
        scan_items(text, &mut items);
        items
    }

    fn ingredient(name: &str, quantity: Option<&str>, unit: Option<&str>) -> MenuItem {
        MenuItem::Ingredient {
            name: name.to_string(),
            quantity: quantity.map(str::to_string),
            unit: unit.map(str::to_string),
        }
    }

    fn reference(path: &str, quantity: Option<&str>, unit: Option<&str>) -> MenuItem {
        MenuItem::RecipeReference {
            name: path.rsplit('/').next().unwrap().to_string(),
            path: path.to_string(),
            quantity: quantity.map(str::to_string),
            unit: unit.map(str::to_string),
            scale: None,
        }
    }

    fn text(t: &str) -> MenuItem {
        MenuItem::Text {
            text: t.to_string(),
        }
    }

    fn note(t: &str) -> MenuItem {
        MenuItem::Note {
            text: t.to_string(),
        }
    }

    #[test]
    fn ingredients_with_connecting_text() {
        assert_eq!(
            scan("@oats{1%cup} with @milk{1/2%cup}"),
            vec![
                ingredient("oats", Some("1"), Some("cup")),
                text(" with "),
                ingredient("milk", Some("1/2"), Some("cup")),
            ]
        );
    }

    #[test]
    fn multi_word_braced_ingredient() {
        assert_eq!(
            scan("@maple syrup{2%tbsp}"),
            vec![ingredient("maple syrup", Some("2"), Some("tbsp"))]
        );
    }

    #[test]
    fn unbraced_ingredient_is_one_word() {
        assert_eq!(
            scan("@salt and @black pepper{}"),
            vec![
                ingredient("salt", None, None),
                text(" and "),
                ingredient("black pepper", None, None),
            ]
        );
    }

    #[test]
    fn unbraced_ingredient_drops_trailing_period() {
        assert_eq!(scan("@eggs."), vec![ingredient("eggs", None, None), text(".")]);
    }

    #[test]
    fn quantity_without_unit() {
        assert_eq!(scan("@eggs{2}"), vec![ingredient("eggs", Some("2"), None)]);
    }

    #[test]
    fn trailing_note_in_parentheses_is_skipped() {
        assert_eq!(
            scan("@pasta{300%g}(cooked)"),
            vec![ingredient("pasta", Some("300"), Some("g"))]
        );
    }

    #[test]
    fn recipe_reference_strips_dot_slash_and_extension() {
        assert_eq!(
            scan("@./Breakfast/Shakshuka.cook{2}"),
            vec![reference("Breakfast/Shakshuka", Some("2"), None)]
        );
    }

    #[test]
    fn recipe_reference_with_spaces_and_servings() {
        assert_eq!(
            scan("@./Breakfast/Easy Pancakes{3%servings}"),
            vec![reference("Breakfast/Easy Pancakes", Some("3"), Some("servings"))]
        );
    }

    #[test]
    fn unbraced_recipe_reference() {
        assert_eq!(scan("@./lamb-chops"), vec![reference("lamb-chops", None, None)]);
    }

    #[test]
    fn parent_relative_reference_keeps_dot_dot() {
        assert_eq!(
            scan("@../Shared/Bread{}"),
            vec![reference("../Shared/Bread", None, None)]
        );
    }

    #[test]
    fn modifiers_are_ignored() {
        assert_eq!(
            scan("@?parsley{} @&./Sauce{}"),
            vec![
                ingredient("parsley", None, None),
                reference("Sauce", None, None),
            ]
        );
    }

    #[test]
    fn inline_comment_becomes_note() {
        assert_eq!(
            scan("@./Risotto{} -- use leftover stock"),
            vec![
                reference("Risotto", None, None),
                note("use leftover stock"),
            ]
        );
    }

    #[test]
    fn whole_line_comment_is_note() {
        assert_eq!(scan("-- save leftovers"), vec![note("save leftovers")]);
    }

    #[test]
    fn lone_at_sign_is_text() {
        assert_eq!(scan("meet @ noon"), vec![text("meet @ noon")]);
    }

    #[test]
    fn whitespace_collapses() {
        assert_eq!(scan("  a   b  "), vec![text(" a b ")]);
    }

    #[test]
    fn blank_text_is_dropped() {
        assert_eq!(scan("   "), vec![]);
    }
}
```

Add `mod parse;` under `mod model;` in `src/menu/mod.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib menu::parse`
Expected: FAIL to compile with "cannot find function `scan_items`".

- [ ] **Step 3: Implement the scanner**

Insert between the `use` line and `#[cfg(test)]` in `src/menu/parse.rs`:

```rust
/// Appends the items found in `text` (one line, bullet and `\` already
/// removed) to `items`.
fn scan_items(text: &str, items: &mut Vec<MenuItem>) {
    let mut plain_start = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(note) = rest.strip_prefix("--") {
            push_text(&text[plain_start..i], items);
            let note = note.trim();
            if !note.is_empty() {
                items.push(MenuItem::Note {
                    text: note.to_string(),
                });
            }
            return;
        }
        if let Some(after_at) = rest.strip_prefix('@') {
            if let Some((item, len)) = parse_component(after_at) {
                push_text(&text[plain_start..i], items);
                items.push(item);
                i += 1 + len;
                plain_start = i;
                continue;
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    push_text(&text[plain_start..], items);
}

/// Pushes `text` as a [`MenuItem::Text`] unless it is blank.
fn push_text(text: &str, items: &mut Vec<MenuItem>) {
    if text.trim().is_empty() {
        return;
    }
    let mut collapsed = String::new();
    if text.starts_with(char::is_whitespace) {
        collapsed.push(' ');
    }
    collapsed.push_str(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    if text.ends_with(char::is_whitespace) {
        collapsed.push(' ');
    }
    items.push(MenuItem::Text { text: collapsed });
}

/// Parses the component after an `@`, returning it and the bytes consumed.
fn parse_component(s: &str) -> Option<(MenuItem, usize)> {
    // Modifiers (`@?`, `@+`, `@-`, `@&`) don't change what is listed.
    let body = s.trim_start_matches(['?', '+', '-', '&']);
    let modifiers = s.len() - body.len();

    let (name, amount, len) = match braced(body) {
        Some((name, amount, mut len)) => {
            // Skip a trailing note such as `(cooked)`.
            if body[len..].starts_with('(') {
                if let Some(close) = body[len..].find(')') {
                    len += close + 1;
                }
            }
            (name, Some(amount), len)
        }
        None => {
            let len = single_word_len(body);
            if len == 0 {
                return None;
            }
            (&body[..len], None, len)
        }
    };

    let (quantity, unit) = split_amount(amount);
    let item = if name.starts_with("./") || name.starts_with("../") {
        recipe_reference(name, quantity, unit)
    } else {
        MenuItem::Ingredient {
            name: name.to_string(),
            quantity,
            unit,
        }
    };
    Some((item, modifiers + len))
}

/// Matches `name{amount}`, returning the name, the text inside the braces,
/// and the bytes consumed. The name may contain spaces but no other
/// component markers.
fn braced(body: &str) -> Option<(&str, &str, usize)> {
    let open = body.find('{')?;
    let name = &body[..open];
    if name.trim().is_empty()
        || name.starts_with(char::is_whitespace)
        || name.contains(['@', '#', '~', '}'])
        || name.contains("--")
    {
        return None;
    }
    let close = open + body[open..].find('}')?;
    Some((name.trim_end(), &body[open + 1..close], close + 1))
}

/// Length of an unbraced, single-word name.
fn single_word_len(body: &str) -> usize {
    let end = body
        .find(|c: char| c.is_whitespace() || ",;:!?(){}".contains(c))
        .unwrap_or(body.len());
    body[..end].trim_end_matches('.').len()
}

/// Splits `quantity%unit`, mapping blank parts to `None`.
fn split_amount(amount: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(amount) = amount else {
        return (None, None);
    };
    let (quantity, unit) = match amount.split_once('%') {
        Some((quantity, unit)) => (quantity, Some(unit)),
        None => (amount, None),
    };
    (non_blank(quantity), unit.and_then(non_blank))
}

fn non_blank(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn recipe_reference(name: &str, quantity: Option<String>, unit: Option<String>) -> MenuItem {
    let path = name.strip_prefix("./").unwrap_or(name);
    let path = path.strip_suffix(".cook").unwrap_or(path);
    MenuItem::RecipeReference {
        name: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        quantity,
        unit,
        scale: None,
    }
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib menu::parse`
Expected: 16 tests PASS. (Dead-code warnings for `scan_items` outside tests are expected until Task 3.)

- [ ] **Step 5: Commit**

```bash
git add src/menu/parse.rs src/menu/mod.rs
git commit -m "feat(menu): scan menu lines into items"
```

---

## Task 3: Line-level parser (`Menu::parse`)

**Files:**
- Modify: `src/menu/parse.rs`, `src/model/metadata.rs:237`, `src/model/mod.rs`

- [ ] **Step 1: Expose `parse_yaml_content` crate-wide**

In `src/model/metadata.rs` change `pub(super) fn parse_yaml_content` to `pub(crate) fn parse_yaml_content`.

In `src/model/mod.rs`, extend the crate-internal re-export:

```rust
pub(crate) use metadata::{parse_yaml_content, value_as_list, value_candidate_strings};
```

- [ ] **Step 2: Write failing tests**

Append inside the `tests` module of `src/menu/parse.rs`:

```rust
    use super::super::model::{Menu, MenuMeal, MenuSection};
    use indoc::indoc;

    fn meal(meal_type: Option<&str>, time: Option<&str>, items: Vec<MenuItem>) -> MenuMeal {
        MenuMeal {
            meal_type: meal_type.map(str::to_string),
            time: time.map(str::to_string),
            items,
        }
    }

    #[test]
    fn parses_weekly_plan() {
        // CookCLI seed/Weekly Plan.menu, first two days.
        let content = indoc! {r"
            ---
            servings: 2
            ---

            ==Saturday (2026-03-07)==

            Breakfast: \
            - @oats{1%cup} with @milk{1/2%cup} and @honey{1%tbsp}

            Lunch: \
            - @./lamb-chops{}

            -- save leftover lamb for sandwiches

            ==Sunday (2026-03-08)==

            Snacks: \
            - @almonds{50%g} \
            - @dark chocolate{30%g}
        "};

        let menu = Menu::parse(content, "Weekly Plan");

        assert_eq!(menu.name, "Weekly Plan");
        assert_eq!(menu.metadata.servings(), Some(2));
        assert_eq!(
            menu.sections,
            vec![
                MenuSection {
                    name: Some("Saturday (2026-03-07)".to_string()),
                    date: Some("2026-03-07".to_string()),
                    meals: vec![
                        meal(
                            Some("Breakfast"),
                            None,
                            vec![
                                ingredient("oats", Some("1"), Some("cup")),
                                text(" with "),
                                ingredient("milk", Some("1/2"), Some("cup")),
                                text(" and "),
                                ingredient("honey", Some("1"), Some("tbsp")),
                            ]
                        ),
                        meal(
                            Some("Lunch"),
                            None,
                            vec![
                                reference("lamb-chops", None, None),
                                note("save leftover lamb for sandwiches"),
                            ]
                        ),
                    ],
                },
                MenuSection {
                    name: Some("Sunday (2026-03-08)".to_string()),
                    date: Some("2026-03-08".to_string()),
                    meals: vec![meal(
                        Some("Snacks"),
                        None,
                        vec![
                            ingredient("almonds", Some("50"), Some("g")),
                            ingredient("dark chocolate", Some("30"), Some("g")),
                        ]
                    )],
                },
            ]
        );
    }

    #[test]
    fn title_overrides_fallback_name() {
        let menu = Menu::parse("---\ntitle: Summer Week\n---\n= Day 1\n", "file");

        assert_eq!(menu.name, "Summer Week");
    }

    #[test]
    fn section_without_date() {
        let menu = Menu::parse("== Day 1 ==\nDinner:\n- @./Soup{}\n", "m");

        assert_eq!(menu.sections[0].name.as_deref(), Some("Day 1"));
        assert_eq!(menu.sections[0].date, None);
    }

    #[test]
    fn bare_date_header() {
        let menu = Menu::parse("= 2026-06-24 Dinner\n@./Stew{}\n", "m");

        assert_eq!(menu.sections[0].date.as_deref(), Some("2026-06-24"));
        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![reference("Stew", None, None)])]
        );
    }

    #[test]
    fn empty_sections_are_kept() {
        let menu = Menu::parse("= Day 1\n= Day 2\n@eggs{}\n", "m");

        assert_eq!(menu.sections.len(), 2);
        assert!(menu.sections[0].meals.is_empty());
    }

    #[test]
    fn meal_header_with_time() {
        let menu = Menu::parse("= Day 1\nBreakfast (08:30):\n- @eggs{2}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Breakfast"),
                Some("08:30"),
                vec![ingredient("eggs", Some("2"), None)]
            )]
        );
    }

    #[test]
    fn meal_header_with_inline_items() {
        let menu = Menu::parse("= Day 1\nBreakfast: @eggs{2}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Breakfast"),
                None,
                vec![ingredient("eggs", Some("2"), None)]
            )]
        );
    }

    #[test]
    fn colon_without_following_space_is_not_a_header() {
        let menu = Menu::parse("= Day 1\nSee https://example.com\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![text("See https://example.com")])]
        );
    }

    #[test]
    fn note_with_colon_is_not_a_header() {
        let menu = Menu::parse("= Day 1\n-- tip: prep the night before\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![note("tip: prep the night before")])]
        );
    }

    #[test]
    fn empty_meals_are_dropped() {
        let menu = Menu::parse("= Day 1\nBreakfast:\nLunch:\n- @./Soup{}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(Some("Lunch"), None, vec![reference("Soup", None, None)])]
        );
    }

    #[test]
    fn content_before_first_section_is_unnamed_section() {
        let menu = Menu::parse("@./Soup{}\n= Day 1\n", "m");

        assert_eq!(menu.sections[0].name, None);
        assert_eq!(menu.sections[1].name.as_deref(), Some("Day 1"));
    }

    #[test]
    fn block_comments_are_removed() {
        let menu = Menu::parse("= Day 1\n[- @./Hidden{}\nstill hidden -]\n@./Shown{}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![reference("Shown", None, None)])]
        );
    }

    #[test]
    fn empty_content() {
        let menu = Menu::parse("", "m");

        assert_eq!(menu.name, "m");
        assert!(menu.sections.is_empty());
    }
```

- [ ] **Step 3: Run tests to verify they fail**

Run: `cargo test --lib menu::parse`
Expected: FAIL to compile with "no function or associated item named `parse` found for struct `Menu`".

- [ ] **Step 4: Implement the parser**

Replace the `use super::model::MenuItem;` line in `src/menu/parse.rs` with:

```rust
use super::model::{Menu, MenuItem, MenuMeal, MenuSection};
use crate::model::{parse_yaml_content, Metadata};
use regex::Regex;
use std::sync::OnceLock;

impl Menu {
    /// Parses menu `content`.
    ///
    /// `fallback_name` is used when the frontmatter has no `title`, typically
    /// the file stem. Parsing never fails: content that isn't recognised is
    /// kept as [`MenuItem::Text`].
    ///
    /// # Examples
    ///
    /// ```
    /// use cooklang_find::Menu;
    ///
    /// let menu = Menu::parse("= Monday (2026-03-09)\nDinner:\n- @./Risotto{}\n", "Week");
    /// assert_eq!(menu.dates(), vec!["2026-03-09"]);
    /// ```
    pub fn parse(content: &str, fallback_name: &str) -> Menu {
        let (metadata, body) = split_frontmatter(content);
        let body = strip_block_comments(body);
        let name = metadata
            .title()
            .map(str::to_string)
            .unwrap_or_else(|| fallback_name.to_string());

        let mut builder = Builder::default();
        for line in body.lines() {
            builder.line(line);
        }

        Menu {
            name,
            metadata,
            sections: builder.finish(),
        }
    }
}

/// Splits leading `---` YAML frontmatter from the body.
fn split_frontmatter(content: &str) -> (Metadata, &str) {
    let mut lines = content.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (Metadata::default(), content);
    };
    if first.trim() != "---" {
        return (Metadata::default(), content);
    }
    let mut offset = first.len();
    for line in lines {
        if line.trim() == "---" {
            let metadata = parse_yaml_content(&content[first.len()..offset]).unwrap_or_default();
            return (metadata, &content[offset + line.len()..]);
        }
        offset += line.len();
    }
    (Metadata::default(), content)
}

fn strip_block_comments(body: &str) -> std::borrow::Cow<'_, str> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)\[-.*?-\]").unwrap())
        .replace_all(body, "")
}

fn extract_date(header: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap())
        .find(header)
        .map(|m| m.as_str().to_string())
}

/// A `Breakfast (08:30):` heading and whatever follows it on the line.
struct MealHeader<'a> {
    meal_type: String,
    time: Option<String>,
    rest: &'a str,
}

impl<'a> MealHeader<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^([^@:()]+?)\s*(?:\((\d{1,2}:\d{2})\))?\s*:(?:\s|$)").unwrap()
        });
        if line.starts_with("--") {
            return None;
        }
        let caps = re.captures(line)?;
        let meal_type = caps[1].trim();
        if meal_type.is_empty() {
            return None;
        }
        Some(MealHeader {
            meal_type: meal_type.to_string(),
            time: caps.get(2).map(|m| m.as_str().to_string()),
            rest: &line[caps[0].len()..],
        })
    }
}

/// Accumulates sections and meals while lines are fed in.
#[derive(Default)]
struct Builder {
    sections: Vec<MenuSection>,
    section: MenuSection,
    meal: MenuMeal,
}

impl Builder {
    fn line(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.starts_with('=') {
            self.start_section(trimmed.trim_matches('=').trim());
            return;
        }

        let line = trimmed.strip_suffix('\\').unwrap_or(trimmed).trim_end();
        let line = strip_bullet(line);
        match MealHeader::parse(line) {
            Some(header) => {
                self.flush_meal();
                self.meal.meal_type = Some(header.meal_type);
                self.meal.time = header.time;
                scan_items(header.rest, &mut self.meal.items);
            }
            None => scan_items(line, &mut self.meal.items),
        }
    }

    fn start_section(&mut self, name: &str) {
        self.flush_section();
        self.section = MenuSection {
            name: (!name.is_empty()).then(|| name.to_string()),
            date: extract_date(name),
            meals: Vec::new(),
        };
    }

    fn flush_meal(&mut self) {
        let meal = std::mem::take(&mut self.meal);
        if !meal.items.is_empty() {
            self.section.meals.push(meal);
        }
    }

    fn flush_section(&mut self) {
        self.flush_meal();
        let section = std::mem::take(&mut self.section);
        if section.name.is_some() || !section.meals.is_empty() {
            self.sections.push(section);
        }
    }

    fn finish(mut self) -> Vec<MenuSection> {
        self.flush_section();
        self.sections
    }
}

/// Removes a leading `- ` list bullet, leaving `--` comments intact.
fn strip_bullet(line: &str) -> &str {
    if line.starts_with("--") {
        return line;
    }
    line.strip_prefix('-').map_or(line, str::trim_start)
}
```

- [ ] **Step 5: Run tests to verify they pass**

Run: `cargo test --lib menu::parse`
Expected: 29 tests PASS.

- [ ] **Step 6: Commit**

```bash
git add src/menu/parse.rs src/model/metadata.rs src/model/mod.rs
git commit -m "feat(menu): parse menu content into sections and meals"
```

---

## Task 4: Scale resolution

**Files:**
- Create: `src/menu/scale.rs`
- Modify: `src/menu/mod.rs`

- [ ] **Step 1: Write failing tests in `src/menu/scale.rs`**

```rust
//! Resolves `{target%unit}` on menu recipe references into multipliers,
//! per the Cooklang spec's "Scaling Referenced Recipes".

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{Menu, MenuItem};
    use camino::Utf8PathBuf;
    use std::fs;
    use tempfile::TempDir;

    fn scales(menu: &Menu) -> Vec<Option<f64>> {
        menu.recipe_references()
            .into_iter()
            .map(|item| match item {
                MenuItem::RecipeReference { scale, .. } => *scale,
                _ => unreachable!(),
            })
            .collect()
    }

    fn recipes_dir() -> (TempDir, Utf8PathBuf) {
        let temp = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        fs::write(dir.join("Pancakes.cook"), "---\nservings: 2\n---\n@flour{}\n").unwrap();
        fs::write(dir.join("Stock.cook"), "---\nyield: 500%ml\n---\n@bones{}\n").unwrap();
        (temp, dir)
    }

    fn resolve(line: &str, dir: &Utf8PathBuf, menu_scale: f64) -> Vec<Option<f64>> {
        let mut menu = Menu::parse(line, "m");
        menu.resolve_scales(&[dir], menu_scale);
        scales(&menu)
    }

    #[test]
    fn no_quantity_is_one() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{}", &dir, 1.0), vec![Some(1.0)]);
    }

    #[test]
    fn bare_number_is_raw_multiplier() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{3}", &dir, 1.0), vec![Some(3.0)]);
    }

    #[test]
    fn fraction_is_parsed() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{1/2}", &dir, 1.0), vec![Some(0.5)]);
    }

    #[test]
    fn servings_divide_by_recipe_servings() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{10%servings}", &dir, 1.0), vec![Some(5.0)]);
    }

    #[test]
    fn yield_with_matching_unit_divides() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Stock{1000%ML}", &dir, 1.0), vec![Some(2.0)]);
    }

    #[test]
    fn yield_with_other_unit_is_raw() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Stock{2%l}", &dir, 1.0), vec![Some(2.0)]);
    }

    #[test]
    fn missing_recipe_is_raw() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Nope{4%servings}", &dir, 1.0), vec![Some(4.0)]);
    }

    #[test]
    fn non_numeric_quantity_is_one() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{some}", &dir, 1.0), vec![Some(1.0)]);
    }

    #[test]
    fn menu_scale_multiplies() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{10%servings}", &dir, 2.0), vec![Some(10.0)]);
    }

    #[test]
    fn parse_number_handles_mixed_fractions() {
        assert_eq!(parse_number("1 1/2"), Some(1.5));
        assert_eq!(parse_number("=2"), Some(2.0));
        assert_eq!(parse_number("2-3"), None);
        assert_eq!(parse_number("1/0"), None);
    }
}
```

Add `mod scale;` under `mod parse;` in `src/menu/mod.rs`.

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib menu::scale`
Expected: FAIL to compile with "no method named `resolve_scales`".

- [ ] **Step 3: Implement**

Insert above `#[cfg(test)]` in `src/menu/scale.rs`:

```rust
use super::model::{Menu, MenuItem};
use crate::fetcher::get_recipe;
use crate::model::Metadata;
use camino::Utf8Path;
use serde_yaml::Value;
use std::collections::HashMap;

impl Menu {
    /// Fills `scale` on every [`MenuItem::RecipeReference`].
    ///
    /// The reference's `{target%unit}` decides the multiplier:
    ///
    /// - nothing or a non-numeric target — 1
    /// - no unit — the target itself (`{2}` = ×2)
    /// - `servings` — target ÷ the referenced recipe's `servings`
    /// - any other unit — target ÷ the referenced recipe's `yield` value,
    ///   when the units match (case-insensitive)
    ///
    /// When the referenced recipe is missing or lacks the metadata, the
    /// target is used as a raw multiplier. The result is multiplied by
    /// `menu_scale`. Referenced recipes are looked up as `<path>.cook` in
    /// `base_dirs`.
    pub fn resolve_scales<P: AsRef<Utf8Path>>(&mut self, base_dirs: &[P], menu_scale: f64) {
        let mut sizes: HashMap<String, RecipeSize> = HashMap::new();
        let items = self
            .sections
            .iter_mut()
            .flat_map(|section| &mut section.meals)
            .flat_map(|meal| &mut meal.items);
        for item in items {
            if let MenuItem::RecipeReference {
                path,
                quantity,
                unit,
                scale,
                ..
            } = item
            {
                let size = sizes
                    .entry(path.clone())
                    .or_insert_with(|| RecipeSize::load(base_dirs, path));
                *scale = Some(scale_factor(quantity.as_deref(), unit.as_deref(), size) * menu_scale);
            }
        }
    }
}

/// What a referenced recipe declares about its own size.
#[derive(Default)]
struct RecipeSize {
    servings: Option<f64>,
    yield_amount: Option<(f64, String)>,
}

impl RecipeSize {
    fn load<P: AsRef<Utf8Path>>(base_dirs: &[P], path: &str) -> Self {
        let file = format!("{path}.cook");
        match get_recipe(base_dirs.iter().map(|d| d.as_ref()), Utf8Path::new(&file)) {
            Ok(entry) => Self::from_metadata(entry.metadata()),
            Err(_) => Self::default(),
        }
    }

    fn from_metadata(metadata: &Metadata) -> Self {
        RecipeSize {
            servings: metadata.get("servings").and_then(value_number),
            yield_amount: metadata
                .get("yield")
                .and_then(Value::as_str)
                .and_then(parse_yield),
        }
    }
}

fn scale_factor(quantity: Option<&str>, unit: Option<&str>, size: &RecipeSize) -> f64 {
    let Some(target) = quantity.and_then(parse_number) else {
        return 1.0;
    };
    match unit {
        None => target,
        Some(unit) if unit.eq_ignore_ascii_case("servings") || unit.eq_ignore_ascii_case("serving") => {
            match size.servings {
                Some(base) if base > 0.0 => target / base,
                _ => target,
            }
        }
        Some(unit) => match &size.yield_amount {
            Some((base, base_unit)) if base_unit.eq_ignore_ascii_case(unit) && *base > 0.0 => {
                target / base
            }
            _ => target,
        },
    }
}

/// Parses `2`, `1.5`, `1/2`, `1 1/2`, and a leading `=` (fixed quantity).
fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim().trim_start_matches('=').trim();
    let mut total = 0.0;
    let mut any = false;
    for part in s.split_whitespace() {
        total += match part.split_once('/') {
            Some((numerator, denominator)) => {
                let denominator: f64 = denominator.parse().ok()?;
                if denominator == 0.0 {
                    return None;
                }
                numerator.parse::<f64>().ok()? / denominator
            }
            None => part.parse::<f64>().ok()?,
        };
        any = true;
    }
    any.then_some(total)
}

fn value_number(value: &Value) -> Option<f64> {
    value
        .as_f64()
        .or_else(|| value.as_str().and_then(parse_number))
}

/// Parses `yield` metadata of the form `VALUE%UNIT`, e.g. `500%ml`.
fn parse_yield(s: &str) -> Option<(f64, String)> {
    let (value, unit) = s.split_once('%')?;
    let unit = unit.trim();
    if unit.is_empty() {
        return None;
    }
    Some((parse_number(value)?, unit.to_string()))
}
```

- [ ] **Step 4: Run tests to verify they pass**

Run: `cargo test --lib menu::scale`
Expected: 10 tests PASS.

- [ ] **Step 5: Commit**

```bash
git add src/menu/scale.rs src/menu/mod.rs
git commit -m "feat(menu): resolve recipe reference scales"
```

---

## Task 5: `RecipeEntry::menu()` and crate exports

**Files:**
- Modify: `src/model/recipe_entry.rs` (after `is_menu`, ~line 331), `src/lib.rs:47-62`

- [ ] **Step 1: Write failing test**

Append to `src/menu/mod.rs`'s `tests` module:

```rust
    #[test]
    fn recipe_entry_menu_parses_menu_files() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= Day 1 (2026-06-24)\nDinner:\n- @./Stew{}\n");
        write_file(&dir, "stew.cook", "@beef{}\n");

        let menu = RecipeEntry::from_path(dir.join("week.menu"))
            .unwrap()
            .menu()
            .unwrap()
            .unwrap();
        let not_menu = RecipeEntry::from_path(dir.join("stew.cook")).unwrap().menu();

        assert_eq!(menu.name, "week");
        assert_eq!(menu.dates(), vec!["2026-06-24"]);
        assert!(not_menu.is_none());
    }
```

- [ ] **Step 2: Run test to verify it fails**

Run: `cargo test --lib menu::tests::recipe_entry_menu_parses_menu_files`
Expected: FAIL to compile with "no method named `menu`".

- [ ] **Step 3: Implement**

In `src/model/recipe_entry.rs`, add `use crate::menu::Menu;` to the imports and insert after `is_menu()`:

```rust
    /// Parses this entry as a [`Menu`].
    ///
    /// Returns `None` when the entry is not a `.menu` file.
    ///
    /// # Errors
    ///
    /// Returns `RecipeEntryError::IoError` if the file cannot be read.
    pub fn menu(&self) -> Option<Result<Menu, RecipeEntryError>> {
        if !self.is_menu() {
            return None;
        }
        let fallback_name = self.name().clone().unwrap_or_default();
        Some(self.content().map(|content| Menu::parse(&content, &fallback_name)))
    }
```

In `src/lib.rs`, change the `menu` module doc and re-export:

```rust
/// Menu files: discovery by date and structured parsing.
pub mod menu;
```

```rust
pub use menu::{list_menus_for_date, Menu, MenuItem, MenuMeal, MenuSection};
```

- [ ] **Step 4: Run all tests**

Run: `cargo test`
Expected: all tests PASS, including the `Menu::parse` doctest.

- [ ] **Step 5: Commit**

```bash
git add src/model/recipe_entry.rs src/lib.rs src/menu/mod.rs
git commit -m "feat(menu): add RecipeEntry::menu and export Menu types"
```

---

## Task 6: FFI bindings and docs

**Files:**
- Modify: `src/ffi.rs` (imports at line 7; types after `FfiDirListing` ~line 418; functions after `list_menus_for_date` ~line 557; tests module), `BINDINGS.md`

- [ ] **Step 1: Write failing tests**

Append to the `tests` module in `src/ffi.rs`:

```rust
    #[test]
    fn test_parse_menu_content() {
        let content = indoc! {r"
            ---
            title: Week
            ---
            = Mon (2026-03-09)
            Dinner (19:00):
            - @./Mains/Risotto{2} with @parmesan{30%g}
            -- grate fresh
            = Tue (2026-03-10)
            Dinner:
            - @./Mains/Risotto{}
        "};

        let menu = parse_menu_content(content.to_string(), "fallback".to_string());

        assert_eq!(menu.name, "Week");
        assert_eq!(menu.dates, vec!["2026-03-09", "2026-03-10"]);
        assert_eq!(menu.first_date.as_deref(), Some("2026-03-09"));
        assert_eq!(menu.last_date.as_deref(), Some("2026-03-10"));
        assert_eq!(menu.recipe_references.len(), 1);
        let meal = &menu.sections[0].meals[0];
        assert_eq!(meal.meal_type.as_deref(), Some("Dinner"));
        assert_eq!(meal.time.as_deref(), Some("19:00"));
        assert!(matches!(
            &meal.items[0],
            FfiMenuItem::RecipeReference { path, quantity, scale: None, .. }
                if path == "Mains/Risotto" && quantity.as_deref() == Some("2")
        ));
        assert!(matches!(&meal.items[3], FfiMenuItem::Note { text } if text == "grate fresh"));
    }

    #[test]
    fn test_parse_menu_resolves_scales() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().to_str().unwrap();
        create_test_recipe(dir, "Pancakes", "---\nservings: 2\n---\n@flour{}\n");
        let menu_path = format!("{dir}/week.menu");
        fs::write(&menu_path, "= Day 1\n@./Pancakes{6%servings}\n").unwrap();

        let menu = parse_menu(menu_path, vec![dir.to_string()], 1.0).unwrap();

        assert!(matches!(
            &menu.recipe_references[0],
            FfiMenuItem::RecipeReference { scale: Some(s), .. } if *s == 3.0
        ));
    }

    #[test]
    fn test_parse_menu_rejects_recipe_files() {
        let temp_dir = TempDir::new().unwrap();
        let dir = temp_dir.path().to_str().unwrap();
        let path = create_test_recipe(dir, "Pancakes", "@flour{}\n");

        let result = parse_menu(path, vec![dir.to_string()], 1.0);

        assert!(matches!(result, Err(CooklangError::MenuError { .. })));
    }
```

- [ ] **Step 2: Run tests to verify they fail**

Run: `cargo test --lib ffi::tests::test_parse_menu`
Expected: FAIL to compile with "cannot find function `parse_menu_content`".

- [ ] **Step 3: Implement types**

Change the menu import in `src/ffi.rs`:

```rust
use crate::menu::{
    list_menus_for_date as list_menus_for_date_internal, Menu, MenuError, MenuItem, MenuMeal,
    MenuSection,
};
```

Insert after the `FfiDirListing` struct:

```rust
/// FFI-safe representation of a parsed `.menu` file.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiMenu {
    /// Frontmatter title, else the file name
    pub name: String,
    /// Menu frontmatter
    pub metadata: FfiMetadata,
    /// Sections (usually days) in file order
    pub sections: Vec<FfiMenuSection>,
    /// Distinct section dates in file order
    pub dates: Vec<String>,
    /// Earliest section date
    pub first_date: Option<String>,
    /// Latest section date
    pub last_date: Option<String>,
    /// Recipe references, deduplicated by path, in first-seen order
    pub recipe_references: Vec<FfiMenuItem>,
}

/// A section (usually a day) of a menu.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiMenuSection {
    /// Header text; `None` for content before the first header
    pub name: Option<String>,
    /// First `YYYY-MM-DD` in the header
    pub date: Option<String>,
    /// Meals in file order
    pub meals: Vec<FfiMenuMeal>,
}

/// A meal within a menu section.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiMenuMeal {
    /// e.g. "Breakfast"; `None` for items before the first meal heading
    pub meal_type: Option<String>,
    /// `HH:MM` from a heading like `Breakfast (08:30):`
    pub time: Option<String>,
    /// Items in file order
    pub items: Vec<FfiMenuItem>,
}

/// One entry of a menu meal.
#[derive(Debug, Clone, uniffi::Enum)]
pub enum FfiMenuItem {
    /// A reference to another recipe
    RecipeReference {
        name: String,
        path: String,
        quantity: Option<String>,
        unit: Option<String>,
        scale: Option<f64>,
    },
    /// A loose ingredient
    Ingredient {
        name: String,
        quantity: Option<String>,
        unit: Option<String>,
    },
    /// Connecting text
    Text { text: String },
    /// A `-- comment`
    Note { text: String },
}

impl From<&MenuItem> for FfiMenuItem {
    fn from(item: &MenuItem) -> Self {
        match item.clone() {
            MenuItem::RecipeReference {
                name,
                path,
                quantity,
                unit,
                scale,
            } => FfiMenuItem::RecipeReference {
                name,
                path,
                quantity,
                unit,
                scale,
            },
            MenuItem::Ingredient {
                name,
                quantity,
                unit,
            } => FfiMenuItem::Ingredient {
                name,
                quantity,
                unit,
            },
            MenuItem::Text { text } => FfiMenuItem::Text { text },
            MenuItem::Note { text } => FfiMenuItem::Note { text },
        }
    }
}

impl From<&MenuMeal> for FfiMenuMeal {
    fn from(meal: &MenuMeal) -> Self {
        FfiMenuMeal {
            meal_type: meal.meal_type.clone(),
            time: meal.time.clone(),
            items: meal.items.iter().map(FfiMenuItem::from).collect(),
        }
    }
}

impl From<&MenuSection> for FfiMenuSection {
    fn from(section: &MenuSection) -> Self {
        FfiMenuSection {
            name: section.name.clone(),
            date: section.date.clone(),
            meals: section.meals.iter().map(FfiMenuMeal::from).collect(),
        }
    }
}

impl From<&Menu> for FfiMenu {
    fn from(menu: &Menu) -> Self {
        let range = menu.date_range();
        FfiMenu {
            name: menu.name.clone(),
            metadata: FfiMetadata::from(&menu.metadata),
            sections: menu.sections.iter().map(FfiMenuSection::from).collect(),
            dates: menu.dates().into_iter().map(str::to_string).collect(),
            first_date: range.map(|(first, _)| first.to_string()),
            last_date: range.map(|(_, last)| last.to_string()),
            recipe_references: menu
                .recipe_references()
                .into_iter()
                .map(FfiMenuItem::from)
                .collect(),
        }
    }
}
```

- [ ] **Step 4: Implement functions**

Insert after the `list_menus_for_date` wrapper:

```rust
/// Parses a `.menu` file and resolves its recipe reference scales.
///
/// # Arguments
/// * `path` - Path to the `.menu` file
/// * `base_dirs` - Directories to look up referenced recipes in
/// * `scale` - Multiplier applied to the whole menu (1.0 for as written)
///
/// # Returns
/// The parsed menu, or an error if the file cannot be read or isn't a menu.
#[uniffi::export]
pub fn parse_menu(
    path: String,
    base_dirs: Vec<String>,
    scale: f64,
) -> Result<FfiMenu, CooklangError> {
    let entry = RecipeEntry::from_path(path.clone().into())?;
    let mut menu = entry.menu().ok_or_else(|| CooklangError::MenuError {
        reason: format!("Not a menu file: {path}"),
    })??;
    let dirs: Vec<Utf8PathBuf> = base_dirs.into_iter().map(Utf8PathBuf::from).collect();
    menu.resolve_scales(&dirs, scale);
    Ok(FfiMenu::from(&menu))
}

/// Parses menu content without resolving recipe reference scales.
///
/// # Arguments
/// * `content` - The menu text, including any YAML frontmatter
/// * `name` - Name to use when the frontmatter has no title
#[uniffi::export]
pub fn parse_menu_content(content: String, name: String) -> FfiMenu {
    FfiMenu::from(&Menu::parse(&content, &name))
}
```

- [ ] **Step 5: Run tests**

Run: `cargo test --lib ffi::tests::test_parse_menu`
Expected: 3 tests PASS.

- [ ] **Step 6: Document in `BINDINGS.md`**

Add to the Functions table, after the `countRecipes` row:

```markdown
| `listMenusForDate(baseDirs, date)` | Find `.menu` files with a section header containing `date` |
| `parseMenu(path, baseDirs, scale)` | Parse a `.menu` file into days, meals, and items, resolving recipe reference scales |
| `parseMenuContent(content, name)` | Parse menu text without resolving scales |
```

(Skip the `listMenusForDate` row if it already exists.) Add before `## CI/CD`:

```markdown
#### FfiMenu

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String` | Frontmatter title, else the file name |
| `metadata` | `FfiMetadata` | Menu frontmatter |
| `sections` | `List<FfiMenuSection>` | Sections (usually days) in file order |
| `dates` | `List<String>` | Distinct section dates in file order |
| `firstDate` / `lastDate` | `String?` | Earliest / latest section date |
| `recipeReferences` | `List<FfiMenuItem>` | Recipe references, deduplicated by path |

#### FfiMenuSection

| Field | Type | Description |
|-------|------|-------------|
| `name` | `String?` | Header text, e.g. `Saturday (2026-03-07)`; null before the first header |
| `date` | `String?` | First `YYYY-MM-DD` in the header |
| `meals` | `List<FfiMenuMeal>` | Meals in file order |

#### FfiMenuMeal

| Field | Type | Description |
|-------|------|-------------|
| `mealType` | `String?` | e.g. `Breakfast`; null for items before the first meal heading |
| `time` | `String?` | `HH:MM` from a heading like `Breakfast (08:30):` |
| `items` | `List<FfiMenuItem>` | Items in file order |

#### FfiMenuItem

| Variant | Fields | Description |
|---------|--------|-------------|
| `RecipeReference` | `name`, `path`, `quantity?`, `unit?`, `scale: Double?` | `@./Folder/Recipe{2}`; `path` has no `./` or `.cook`; `scale` is set by `parseMenu` |
| `Ingredient` | `name`, `quantity?`, `unit?` | A loose ingredient; quantity as written |
| `Text` | `text` | Connecting text such as ` with ` |
| `Note` | `text` | A `-- comment` |
```

- [ ] **Step 7: Commit**

```bash
git add src/ffi.rs BINDINGS.md
git commit -m "feat(ffi): expose parsed menus"
```

---

## Task 7: Final verification

- [ ] **Step 1: Format and lint**

Run: `cargo fmt && make lint`
Expected: no diff from `fmt --check`, no clippy warnings.

- [ ] **Step 2: Full test suite, both feature sets**

Run: `cargo test && cargo test --no-default-features`
Expected: all PASS.

- [ ] **Step 3: Docs build**

Run: `cargo doc --no-deps`
Expected: no warnings about broken intra-doc links.

- [ ] **Step 4: Commit any formatting changes**

```bash
git add -A src
git commit -m "style: cargo fmt"
```

(Skip if nothing changed.)
