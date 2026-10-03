# Design: structured `Menu` model

**Date:** 2026-10-03
**Status:** Approved

## Overview

`.menu` files are currently surfaced only as `RecipeEntry` values. Every
consumer re-parses them to show a meal plan:

- CookCLI: `server/handlers/menus.rs` (`GET /api/menus/*path`),
  `web/builders.rs` (HTML template), `server/ui.rs`, and
  `shopping_list::add_menu`, plus `util/menu_scale.rs` for reference scaling.
- iOS: `MealPlanning/Mapper/MealPlanMapper.swift`.

This design adds a `Menu` structure to cooklang-find that gives consumers the
days, meals, and items of a menu, plus helpers for common display needs. Its
shape mirrors CookCLI's existing `/api/menus/*path` response so CookCLI can
migrate onto it with minimal changes and iOS can drop its own mapper.

## Data model

New file `src/menu/model.rs`, re-exported from `menu` and the crate root. All
types derive `Debug, Clone, PartialEq, Serialize`.

```rust
pub struct Menu {
    /// Frontmatter `title`, else the fallback name (file stem).
    pub name: String,
    /// Frontmatter, parsed with the existing `Metadata` type.
    pub metadata: Metadata,
    pub sections: Vec<MenuSection>,
}

pub struct MenuSection {
    /// Section header text with `=` markers trimmed. `None` for content
    /// before the first header.
    pub name: Option<String>,
    /// First `YYYY-MM-DD` found anywhere in the header, as written.
    pub date: Option<String>,
    pub meals: Vec<MenuMeal>,
}

pub struct MenuMeal {
    /// Header text without the trailing `:` and `(HH:MM)`, e.g. "Breakfast".
    /// `None` for items before the first meal header in a section.
    pub meal_type: Option<String>,
    /// `HH:MM` from a header like `Breakfast (08:30):`.
    pub time: Option<String>,
    pub items: Vec<MenuItem>,
}

#[serde(tag = "kind", rename_all = "snake_case")]
pub enum MenuItem {
    RecipeReference {
        /// Last path component, e.g. "Easy Pancakes".
        name: String,
        /// Path relative to the library root, with `./` and `.cook`
        /// stripped (a `.menu` suffix is kept), e.g. "Breakfast/Easy
        /// Pancakes" or "Weekly.menu". To load it, look up `<path>.cook`
        /// (or `<path>` if it ends in `.menu`) under the base dir; not
        /// directly suitable for `get_recipe`, whose extension detection
        /// breaks on dotted names like "Mr. Smith's Stew".
        path: String,
        /// Target quantity as authored inside `{}` (e.g. "2", "1/2").
        quantity: Option<String>,
        /// Target unit as authored (e.g. "servings", "ml").
        unit: Option<String>,
        /// Resolved multiplier. `None` until `Menu::resolve_scales` runs.
        scale: Option<f64>,
    },
    Ingredient {
        name: String,
        quantity: Option<String>,
        unit: Option<String>,
    },
    /// Connecting text such as "with". Whitespace runs collapse to one
    /// space; a single leading/trailing space is kept so consumers can
    /// concatenate items as-is.
    Text { text: String },
    /// `-- comment`, on its own line or at the end of one.
    Note { text: String },
    /// Separates items from different lines of the same meal; serialized as
    /// `{"kind":"line_break"}`. Never a meal's first or last item.
    LineBreak,
}
```

Differences from CookCLI's API shape, intentional:

- `meal_type` is `Option` instead of the synthetic `"Items"` string; CookCLI
  can map `None` to `"Items"` when serializing its response.
- `Text` and `Note` are kept so the HTML view and iOS (which shows notes) can
  render the full menu. CookCLI's JSON API can filter them out.
- Quantities are the authored text; this crate does not format or scale loose
  ingredients.

## Parsing

`Menu::parse(content: &str, fallback_name: &str) -> Menu`. A line scanner,
no `cooklang` dependency, consistent with `menu/mod.rs` and metadata
extraction. Parsing never fails; unrecognised content becomes `Text`.

1. Strip a leading BOM (`\u{feff}`), then YAML frontmatter, parsed with
   `Metadata`. Blank lines, `---` rules (3+ dashes), and deprecated `>>`
   metadata lines are skipped.
2. Section header: trimmed line starting with `=`; name = trim `=` and
   whitespace (`section_name`, shared with `list_menus_for_date`). Starts a new `MenuSection`. Content before the first header
   goes into a section with `name: None` (omitted if empty).
3. Note: trimmed line starting with `--` → `Note` in the current meal.
4. Meal header: line (bullet-less, trailing `\` ignored, not starting with
   `--`) matching `^([^@:]+?)\s*(\((HH:MM)\))?\s*:` followed by
   whitespace or end of line (so `2:30` and `https://` don't count), where
   the rest of the line is empty, starts with `@`, or starts with `--` (so
   `Tip: prep ahead` stays `Text`, while `Dinner: -- eating out` starts a
   "Dinner" meal whose first item is `Note`). Starts a new `MenuMeal`; `(HH:MM)` is extracted into
   `time`, other parentheses stay in the name (`Lunch (packed)`). Items after
   the `:` belong to the new meal (`Breakfast: @eggs{}`).
5. Other lines: tokenise into items. A leading `-` bullet is dropped only
   when followed by whitespace or end of line (`-5 degrees` is text); a
   trailing `\` is dropped. When a line adds items to a meal that already has
   some, a `LineBreak` is inserted first.
   - `@name{qty%unit}` / `@name{}` — name may contain spaces when braces
     follow; `@name` without braces ends at the first whitespace,
     punctuation, or `@`. An empty name (`@./.cook{}`) leaves the text as
     `Text`. Trailing `(note)` after an ingredient is ignored.
   - Name starting with `./` or `../` → `RecipeReference` (strip `./`,
     strip `.cook`; a `.menu` suffix is kept so consumers can tell it's a
     menu). Otherwise → `Ingredient`. One leading modifier
     (`?`, `+`, `-`, `&`) is ignored.
   - `--` outside a component starts a `Note` for the rest of the line.
   - Remaining text between items → `Text` (skipped when blank).
6. Meals with no items are dropped; sections are kept even if empty so day
   lists stay complete.
7. Block comments `[- … -]` are removed before scanning.

Date regex: `\d{4}-\d{2}-\d{2}` not adjacent to other digits, first match. This is consistent with
`list_menus_for_date`'s substring match, so a menu found for a date always
has a section with that `date` (for ISO dates).

## Scale resolution

```rust
impl Menu {
    pub fn resolve_scales<P: AsRef<Utf8Path>>(&mut self, base_dirs: &[P], menu_scale: f64);
}
```

Fills `scale` on every `RecipeReference`, ported from CookCLI's
`menu_scale::reference_scale_factor`, multiplied by `menu_scale`:

- no quantity → 1.0
- non-numeric quantity → 1.0
- number, no unit → raw multiplier
- unit `serving(s)` → target / referenced `servings` (fallback: raw)
- other unit → target / referenced `yield` value when units match
  case-insensitively (fallback: raw)

Quantities follow Cooklang's number grammar: an optional leading `=`, then
one integer (`2`), decimal (`1.5`), fraction (`1/2`), or mixed number
(`1 1/2`). Anything else (`-2`, `1e2`, `inf`, `NaN`, `2 3`, `2-3`, `1/0`) is
non-numeric → 1.0. A fixed `{=2}` is still multiplied by `menu_scale`, as in
CookCLI. The same grammar parses string `servings` and the value of `yield`
(`VALUE%UNIT`); non-finite or non-positive values count as absent.

A referenced recipe is loaded only when the result depends on it (numeric
target with a unit), memoised per call. Paths resolve against `base_dirs`
(the library root), not the menu's folder: for each base dir in turn,
`<path>.cook` then `<path>.menu` (appended explicitly so dotted names aren't
mistaken for extensions); a path already ending in `.menu`
(`@./Weekly.menu{}`) is looked up as-is. Missing recipes fall back silently to raw.

The caller must pass a finite `menu_scale > 0`; `ffi::parse_menu` returns
`CooklangError::MenuError` otherwise.

## Helpers

```rust
impl Menu {
    pub fn sections_for_date(&self, date: &str) -> Vec<&MenuSection>;
    pub fn dates(&self) -> Vec<&str>;                    // distinct, file order
    pub fn date_range(&self) -> Option<(&str, &str)>;    // lexical min/max
    pub fn recipe_references(&self) -> Vec<&MenuItem>;   // deduped by path, first occurrence
}
```

## Entry points

- `Menu::parse(content, fallback_name)`.
- `RecipeEntry::menu(&self) -> Option<Result<Menu, RecipeEntryError>>` —
  `None` when `!is_menu()`; uses `content()` and `name()`.

## FFI

In `src/ffi.rs`, following existing record/enum conventions:

- Records `FfiMenu`, `FfiMenuSection`, `FfiMenuMeal`; `FfiMenuItem` as a
  `uniffi::Enum`. `FfiMenu.metadata` uses the existing metadata
  representation.
- `parse_menu(path: String, base_dirs: Vec<String>, scale: f64) -> Result<FfiMenu, CooklangError>`
  — reads, parses, resolves scales.
- `parse_menu_content(content: String, name: String) -> FfiMenu` — no scale
  resolution.
- uniffi records can't carry methods, so `FfiMenu` precomputes the helpers
  as fields: `dates`, `first_date`, `last_date`, `recipe_references`.
  Filtering sections by date is left to the host (a one-line filter).
- `BINDINGS.md` updated.

## Testing

Unit tests with `indoc`/`tempfile`:

- CookCLI seed `Weekly Plan.menu` and a `3 Day Plan` sample as fixtures,
  asserting full structure.
- Headers: `==Saturday (2026-03-07)==`, `= 2026-06-24 Dinner`, `== Day 1 ==`.
- Meal headers with time, with trailing `\`, inline items after `:`.
- References with/without `./`, with/without `.cook`, nested folders,
  `{}`, `{2}`, `{3%servings}`, `{500%ml}`, `{1/2}`.
- Unbraced `@oats`, multi-word braced ingredients, trailing `(cooked)`.
- Notes, block comments, content before first header, empty file.
- `resolve_scales`: servings, yield with matching/mismatched units, missing
  recipe, menu scale multiplication.
- Helpers: `sections_for_date`, `dates` dedup, `date_range`, dedup in
  `recipe_references`.

## Out of scope

- Scaling or formatting loose ingredient quantities.
- Date parsing, weekday names, timezone handling.
- Nested sub-references (CookCLI shopping list's `sub_refs`).
- Migrating CookCLI and iOS to the new model (follow-up PRs).
