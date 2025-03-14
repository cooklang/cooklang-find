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
cooklang-find = "0.1.0"
```

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
- `RecipeError`: Problems with recipe parsing or reading
- `TreeError`: Errors in building recipe tree structure
- `SearchError`: Issues during recipe search

## Contributing

Contributions are welcome! Please feel free to submit a Pull Request.

## License

This project is licensed under the MIT License - see the LICENSE file for details.

## Acknowledgments

- [Cooklang](https://cooklang.org/) - The recipe markup language
- [cooklang-rs](https://crates.io/crates/cooklang) - Rust implementation of Cooklang parser