# Design: `RecipeEntry::related_files()`

## Summary

Add a method to `RecipeEntry` that returns all filesystem paths related to a recipe: images (title + step) and recursively referenced recipe files.

## Public API

```rust
impl RecipeEntry {
    /// Returns all file paths related to this recipe.
    ///
    /// Includes:
    /// - Title image (if any)
    /// - Step/section images
    /// - Referenced recipe .cook files (detected via @./path syntax)
    /// - Recursively: related files of referenced recipes
    ///
    /// Returns an empty Vec for content-based recipes.
    /// Missing referenced files are silently skipped.
    /// Cycles are detected and broken automatically.
    pub fn related_files(&self) -> Vec<Utf8PathBuf>
}
```

## Internal Design

### Helper function

```rust
fn collect_related_files(
    path: &Utf8Path,
    visited: &mut HashSet<Utf8PathBuf>,
    result: &mut Vec<Utf8PathBuf>,
)
```

### Algorithm

1. Add recipe path to `visited` set (prevents cycles)
2. Find title image via `find_title_image()` — add to result if found
3. Find step images via `find_step_images()` — add all paths to result
4. Read recipe content, extract `@./path` references via regex
5. For each referenced recipe:
   - Resolve path relative to current recipe's directory
   - Try `.cook` extension if not specified
   - If file exists and not in `visited`: add to result, recurse

### Regex for recipe references

Pattern: `@([./][^{}\s]+)` — captures path after `@` starting with `./`, stopping at whitespace or `{`.

### No caching

The method returns a fresh `Vec` each call. Caching doesn't fit well with:
- Recursive traversal needing mutable `visited` state
- File system state that can change between calls

## Edge Cases

- **Content-based recipes** (`RecipeSource::Content`): return empty `Vec`
- **Missing referenced files**: silently skip
- **Circular references**: `visited` set prevents infinite recursion
- **Deduplication**: `visited` set deduplicates across branches

## FFI

Add to `FfiRecipeEntry`:

```rust
fn related_files(&self) -> Vec<String>
```

## Decisions

- **Regex over parser**: lightweight, no new dependencies, sufficient for `@./path` pattern
- **No caching**: simpler recursion, avoids stale filesystem data
- **Method on RecipeEntry**: consistent with existing API (`title_image()`, `step_images()`)
- **Flat Vec<Utf8PathBuf>**: simple to consume, deduplicated
- **Recursive**: follows referenced recipes to collect their files too
