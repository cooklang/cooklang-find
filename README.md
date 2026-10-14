# cooklang-find

A Rust library for finding and managing [Cooklang](https://cooklang.org/) recipes in the filesystem. This library provides functionality to search, organize, and manage your recipe collection efficiently.

## Features

- **Recipe Search**: Find recipes by name or content across multiple directories
- **Recipe Tree**: Build and navigate a hierarchical structure of your recipe collection
- **Metadata Support**: Parse and access recipe metadata using the new frontmatter format
- **Title Image Support**: Automatically find and associate images with recipes
- **Structured Menus**: Parse `.menu` meal plans into days, meals, and items, with recipe reference scaling
- **Companion Files**: The title and step images that belong to a recipe, and where they go on a rename
- **Recipe References**: `@./path` references resolved with one root-relative rule
- **Error Handling**: Comprehensive error handling with custom error types

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
cooklang-find = "0.9"
```

### iOS (Swift Package Manager)

Add to your Xcode project or `Package.swift`:

```swift
.package(url: "https://github.com/cooklang/cooklang-find", from: "0.5.1")
```

### Android (GitHub Packages)

Add to your `build.gradle.kts`:

```kotlin
implementation("org.cooklang:cooklang-find:0.5.1")
```

See [BINDINGS.md](BINDINGS.md) for detailed mobile integration instructions.

## Usage

### Finding a Recipe

```rust
use cooklang_find::get_recipe;
use std::path::Path;

// Search for a recipe in multiple directories
let recipe_dirs = vec![
    Path::new("~/recipes"),
    Path::new("~/more-recipes")
];

match get_recipe(recipe_dirs, Path::new("pancakes")) {
    Ok(Some(recipe)) => println!("Found recipe: {}", recipe.name),
    Ok(None) => println!("Recipe not found"),
    Err(e) => eprintln!("Error: {}", e),
}
```

### Filter by Metadata

`filter_by_metadata` and `search_with_filter` answer "which recipes have
metadata X" — e.g. "source contains koreanbapsang", "tags lacks Korean",
"cuisine equals Japanese" — **without reading recipe bodies**. This makes
them cheap to run over large collections, e.g. from an AI assistant that
would otherwise have to read every recipe file to answer a metadata
question.

A `MetadataFilter` is built from a small JSON grammar (`where` conditions
are ANDed together):

```rust
use cooklang_find::{filter_by_metadata, search_with_filter, MetadataFilter};
use camino::Utf8Path;

let filter = MetadataFilter::from_json(r#"{
    "where": {
        "source": { "contains": "koreanbapsang" },
        "tags": { "missing": "vegetarian" },
        "cuisine": { "equals": "Korean" }
    },
    "titleContains": "kimchi"
}"#).expect("valid filter JSON");

// Metadata only, no query: reads only frontmatter.
match filter_by_metadata(Utf8Path::new("~/recipes"), &filter) {
    Ok(recipes) => println!("Found {} recipes", recipes.len()),
    Err(e) => eprintln!("Error: {}", e),
}

// Combine with a full-text query; recipe bodies are read only because a
// query was given, same as plain `search`.
match search_with_filter(Utf8Path::new("~/recipes"), "stew", &filter) {
    Ok(recipes) => println!("Found {} recipes", recipes.len()),
    Err(e) => eprintln!("Error: {}", e),
}
```

Each condition names one operator:

| Operator | Meaning |
|----------|---------|
| `{ "contains": "x" }` / `{ "contains": ["x", "y"] }` | Case-insensitive substring match; true if *any* needle matches *any* candidate string of the value |
| `{ "equals": "x" }` | Case-insensitive whole-string equality against any candidate string |
| `{ "has": "x" }` | The value (an array, or comma-separated string, the way `Metadata::tags()` already reads tags) contains `x`, case-insensitively |
| `{ "missing": "x" }` | The inverse of `has`; also true when the key is absent |
| `{ "exists": true \| false }` | Whether the key is present at all |

A key may be dotted (`"source.url"`) to address a value nested inside a
YAML mapping, and both keys and string comparisons are case-insensitive.
For a mapping value like `source: { name: .., url: .. }`, a condition on
the bare key `source` searches every value in the mapping — so
`{"source": {"contains": "koreanbapsang"}}` matches both
`source: https://koreanbapsang.com/x` and
`source: { name: .., url: https://koreanbapsang.com/x }`.

### Building a Recipe Tree

```rust
use cooklang_find::build_tree;
use std::path::Path;

// Build a tree structure of your recipe collection
match build_tree(Path::new("~/recipes")) {
    Ok(tree) => {
        // Access recipes and subdirectories
        for (name, node) in tree.children {
            if let Some(recipe) = node.recipe {
                println!("Found recipe: {}", recipe.name);
            } else {
                println!("Found directory: {}", name);
            }
        }
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

### Searching Recipes

```rust
use cooklang_find::search;
use std::path::Path;

// Search for recipes containing specific text
match search(Path::new("~/recipes"), "pancake") {
    Ok(recipes) => {
        for recipe in recipes {
            println!("Found matching recipe: {}", recipe.name);
        }
    }
    Err(e) => eprintln!("Error: {}", e),
}
```

### Working with Menus

`.menu` files are meal plans: sections (usually days) that hold meals, which
hold recipe references and loose ingredients. Given this menu:

```cooklang
---
title: Week 10
servings: 2
---

== Saturday (2026-03-07) ==

Breakfast (08:30):
- @./Breakfast/Easy Pancakes{4%servings} with @maple syrup{2%tbsp}

Dinner:
- @./Risotto{}
-- save leftovers for lunch

== Sunday (2026-03-08) ==

Lunch:
- @./Risotto{1/2}
```

`Menu::parse` (or `RecipeEntry::menu()` for a `.menu` file) gives:

```text
Menu { name: "Week 10", metadata, sections }
└─ MenuSection { name: "Saturday (2026-03-07)", date: "2026-03-07" }
   ├─ MenuMeal { meal_type: "Breakfast", time: "08:30" }
   │  ├─ RecipeReference { name: "Easy Pancakes", path: "Breakfast/Easy Pancakes",
   │  │                    quantity: "4", unit: "servings", scale }
   │  ├─ Text { text: " with " }
   │  └─ Ingredient { name: "maple syrup", quantity: "2", unit: "tbsp" }
   └─ MenuMeal { meal_type: "Dinner", time: None }
      ├─ RecipeReference { name: "Risotto", path: "Risotto", .. }
      ├─ LineBreak
      └─ Note { text: "save leftovers for lunch" }
```

- `date` is the first `YYYY-MM-DD` in the section header; `time` comes from a
  meal header like `Breakfast (08:30):`.
- Quantities are kept as written (`"1/2"`, `"2"`). Items before the first meal
  header get `meal_type: None`.
- `LineBreak` separates the lines of a meal, so items can be rendered in order.
- `MenuItem` is `#[non_exhaustive]`; match it with a wildcard arm.

```rust
use cooklang_find::{get_recipe, MenuItem};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let entry = get_recipe(vec!["./recipes"], "Week 10")?;
    let mut menu = entry.menu().expect("not a .menu file")?;

    // Turn `{4%servings}`, `{2}`, `{500%ml}` into multipliers using each
    // referenced recipe's own `servings` / `yield`. Paths resolve against the
    // library root; the last argument scales the whole menu.
    menu.resolve_scales(&["./recipes"], 1.0);

    // What's on today?
    for section in menu.sections_for_date("2026-03-07") {
        for meal in &section.meals {
            println!("{}", meal.meal_type.as_deref().unwrap_or("Other"));
            for item in &meal.items {
                match item {
                    MenuItem::RecipeReference { name, scale, .. } => {
                        println!("  {name} ×{}", scale.unwrap_or(1.0))
                    }
                    MenuItem::Ingredient { name, .. } => println!("  {name}"),
                    _ => {}
                }
            }
        }
    }

    // Plan span for a subtitle, and every recipe once (for prefetching).
    let range = menu.date_range(); // Some(("2026-03-07", "2026-03-08"))
    let recipes = menu.recipe_references();
    Ok(())
}
```

To find the menus for a range of days, parsed in one walk, use
`list_menus_between(&[dir], "2026-03-07", "2026-03-08")`; each match carries the
file's `path` and its `menu`.
On iOS and Android, `parseMenu(path, baseDirs, scale)` returns the same
structure as `FfiMenu`, with `dates`, `firstDate`, `lastDate` and
`recipeReferences` precomputed (see [BINDINGS.md](BINDINGS.md)).

### Companion Files

A recipe's images sit next to it and share its stem: `Pasta.jpg` is the
title image, `Pasta.2.jpg` step 2 of a recipe without sections, and
`Pasta.1.2.jpg` section 1, step 2. Steps and sections are one-based.
Extensions are jpg, jpeg, png, webp and heic, in any letter case.

```rust
use cooklang_find::{companion_files, renamed_companions};
use camino::Utf8Path;

let recipe = Utf8Path::new("recipes/Pasta.cook");
for image in companion_files(recipe) {
    println!("{image}");
}

// Planning a rename: the caller moves the files.
for file in renamed_companions(recipe, Utf8Path::new("recipes/Mains/Noodles.cook")) {
    println!("{} -> {}", file.from, file.to);
}
```

### Resolving References

`@./sauces/Pesto{100%g}` in a recipe or menu names another file in the
collection. References resolve against the collection root, never the
referencing file's folder, so the same reference means the same file from
anywhere in the tree. `.cook` is tried before `.menu`; a reference that
leaves the root is `None`.

```rust
use cooklang_find::{resolve_reference, RecipeEntry};
use camino::{Utf8Path, Utf8PathBuf};

let root = Utf8Path::new("recipes");
let pesto = resolve_reference(root, "./sauces/Pesto");

// Everything a recipe depends on, recursively: images, referenced recipes
// and menus, and theirs.
let recipe = RecipeEntry::from_path(Utf8PathBuf::from("recipes/Pasta.cook")).expect("recipe file");
let files = recipe.related_files(root);
```

## Recipe Format

The library supports Cooklang recipes with frontmatter metadata. Example:

```cooklang
---
servings: 4
time: 30 min
cuisine: Italian
---

Prepare the @pasta{500%g} by boiling it in @water{2%l}.
Add @salt{1%tsp} to taste.
```

## Features in Detail

### Recipe Search
- Search by filename or content
- Case-insensitive matching
- Support for multiple search directories
- Priority-based search (first directory match wins)
- Metadata-only filtering (`filter_by_metadata`, `search_with_filter`) that
  never reads recipe bodies unless a text query is also given

### Recipe Tree
- Build hierarchical structure of recipes
- Support for nested directories
- Easy navigation of recipe collection
- Automatic directory creation and management

### Menus
- Sections with dates, meals with optional times, and their items
- Recipe references, loose ingredients, connecting text, notes, line breaks
- Reference scaling per the Cooklang spec (`{2}`, `{4%servings}`, `{500%ml}`)
- Date helpers: `sections_for_date`, `dates`, `date_range`
- Find menus for a date range with `list_menus_between` (or a single day with
  `list_menus_for_date`)

### Metadata Support
- Parse frontmatter metadata
- Access common fields (servings, time, cuisine)
- Support for custom metadata fields
- Cached metadata access for performance

### Images
- Title image: `Name.ext` next to the recipe
- Step images: `Name.N.ext` (step N, one-based, no sections) and
  `Name.S.N.ext` (section S, step N, both one-based); `StepImageCollection`
  keeps the two apart, and section 0 means "no sections"
- Extensions jpg, jpeg, png, webp, heic, matched in any letter case; when a
  slot has several, that order decides
- One folder listing per recipe, shared by `title_image`, `step_images`
  and `companion_files`

## Error Handling

The library provides custom error types for different scenarios:

- `FetchError`: Issues with finding recipes
- `RecipeEntryError`: Problems with recipe parsing or reading
- `TreeError`: Errors in building recipe tree structure
- `SearchError`: Issues during recipe search

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- [Cooklang](https://cooklang.org/) - The recipe markup language
- [cooklang-rs](https://crates.io/crates/cooklang) - Rust implementation of Cooklang parser
