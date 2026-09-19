# cooklang-find

A Rust library for finding and managing [Cooklang](https://cooklang.org/) recipes in the filesystem. This library provides functionality to search, organize, and manage your recipe collection efficiently.

## Features

- **Recipe Search**: Find recipes by name or content across multiple directories
- **Recipe Tree**: Build and navigate a hierarchical structure of your recipe collection
- **Metadata Support**: Parse and access recipe metadata using the new frontmatter format
- **Title Image Support**: Automatically find and associate images with recipes
- **Error Handling**: Comprehensive error handling with custom error types

## Installation

Add this to your `Cargo.toml`:

```toml
[dependencies]
cooklang-find = "0.5.1"
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

### Metadata Support
- Parse frontmatter metadata
- Access common fields (servings, time, cuisine)
- Support for custom metadata fields
- Cached metadata access for performance

### Title Image Support
- Automatic discovery of recipe images
- Support for multiple image formats (jpg, jpeg, png, webp)
- Case-insensitive extension matching
- Automatic association with recipes

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
