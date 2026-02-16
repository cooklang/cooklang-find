# Related Files Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Add a `related_files()` method to `RecipeEntry` that returns all filesystem paths related to a recipe (images + recursively referenced recipe files).

**Architecture:** A public method on `RecipeEntry` delegates to a private recursive helper `collect_related_files()` that takes a mutable `visited` set for cycle detection. Recipe references are detected via regex (`@./path` patterns in content). The `regex` crate is added as a dependency. FFI layer exposes this as `Vec<String>`.

**Tech Stack:** Rust, regex crate, camino (Utf8PathBuf), existing glob-based image discovery

---

### Task 1: Add `regex` dependency

**Files:**
- Modify: `Cargo.toml:21-28` (dependencies section)

**Step 1: Add regex to Cargo.toml**

In `Cargo.toml`, add `regex` to the `[dependencies]` section:

```toml
regex = "1"
```

Add it after the `glob` line (line 23).

**Step 2: Verify it compiles**

Run: `cargo check`
Expected: Compiles successfully with new dependency

**Step 3: Commit**

```bash
git add Cargo.toml Cargo.lock
git commit -m "chore: add regex dependency for recipe reference extraction"
```

---

### Task 2: Write tests for `extract_recipe_references`

**Files:**
- Modify: `src/model/recipe_entry.rs` (add tests at bottom of `mod tests`)

**Step 1: Write tests for the regex extraction function**

Add these tests at the end of the `mod tests` block in `src/model/recipe_entry.rs` (before the final closing `}`):

```rust
    // ========== Tests for extract_recipe_references ==========

    #[test]
    fn test_extract_recipe_references_simple() {
        let content = "Pour @./sauces/Hollandaise{150%g} over the eggs.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs, vec!["./sauces/Hollandaise"]);
    }

    #[test]
    fn test_extract_recipe_references_multiple() {
        let content = "Serve @./sauces/Hollandaise{150%g} with @./sides/Asparagus{200%g}.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs.len(), 2);
        assert!(refs.contains(&"./sauces/Hollandaise".to_string()));
        assert!(refs.contains(&"./sides/Asparagus".to_string()));
    }

    #[test]
    fn test_extract_recipe_references_no_refs() {
        let content = "Add @salt{1%tsp} and @pepper{1%tsp}.";
        let refs = extract_recipe_references(content);
        assert!(refs.is_empty());
    }

    #[test]
    fn test_extract_recipe_references_no_quantity() {
        let content = "Serve with @./sauces/Hollandaise over eggs.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs, vec!["./sauces/Hollandaise"]);
    }

    #[test]
    fn test_extract_recipe_references_deduplicates() {
        let content = "Use @./base/Stock{100%ml} twice and @./base/Stock{200%ml} again.";
        let refs = extract_recipe_references(content);
        assert_eq!(refs, vec!["./base/Stock"]);
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib model::recipe_entry::tests::test_extract_recipe_references -- -v`
Expected: FAIL — `extract_recipe_references` function not found

---

### Task 3: Implement `extract_recipe_references`

**Files:**
- Modify: `src/model/recipe_entry.rs` (add function + use regex)

**Step 1: Add regex import at top of file**

Add to the top of `src/model/recipe_entry.rs`, after the existing `use` statements:

```rust
use regex::Regex;
```

**Step 2: Implement the function**

Add this function after the `parse_image_numbers` function (before `#[cfg(test)]`):

```rust
/// Extracts recipe references from Cooklang content.
///
/// Looks for ingredient references that are relative file paths,
/// matching the pattern `@./path/to/Recipe` with optional quantity `{...}`.
///
/// Returns deduplicated list of referenced paths (without extension).
fn extract_recipe_references(content: &str) -> Vec<String> {
    let re = Regex::new(r"@(\./[^\s\{]+)").unwrap();
    let mut seen = std::collections::HashSet::new();
    let mut refs = Vec::new();
    for cap in re.captures_iter(content) {
        let path = cap[1].to_string();
        if seen.insert(path.clone()) {
            refs.push(path);
        }
    }
    refs
}
```

**Step 3: Run tests to verify they pass**

Run: `cargo test --lib model::recipe_entry::tests::test_extract_recipe_references`
Expected: All 5 tests PASS

**Step 4: Commit**

```bash
git add src/model/recipe_entry.rs
git commit -m "feat: add extract_recipe_references helper for detecting @./path references"
```

---

### Task 4: Write tests for `related_files` (images only, no references)

**Files:**
- Modify: `src/model/recipe_entry.rs` (add tests)

**Step 1: Write tests for image collection via related_files**

Add these tests at the end of `mod tests`:

```rust
    // ========== Tests for related_files ==========

    #[test]
    fn test_related_files_empty() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "simple", "Just a recipe");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();
        assert!(files.is_empty());
    }

    #[test]
    fn test_related_files_with_title_image() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "pasta", "Make pasta");
        let image_path = create_test_image(&temp_dir_path, "pasta", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();
        assert_eq!(files.len(), 1);
        assert_eq!(files[0], image_path);
    }

    #[test]
    fn test_related_files_with_step_images() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "pasta", "Make pasta");
        create_test_image(&temp_dir_path, "pasta.1", "jpg");
        create_test_image(&temp_dir_path, "pasta.2", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();
        assert_eq!(files.len(), 2);
    }

    #[test]
    fn test_related_files_content_based_returns_empty() {
        let recipe = RecipeEntry::from_content("Just content".to_string(), None).unwrap();
        let files = recipe.related_files();
        assert!(files.is_empty());
    }
```

**Step 2: Run tests to verify they fail**

Run: `cargo test --lib model::recipe_entry::tests::test_related_files -- -v`
Expected: FAIL — `related_files` method not found

---

### Task 5: Implement `related_files` (images only)

**Files:**
- Modify: `src/model/recipe_entry.rs` (add method to `impl RecipeEntry` and helper)

**Step 1: Add `related_files` method to `impl RecipeEntry`**

Add this method at the end of the `impl RecipeEntry` block (after `step_images()`):

```rust
    /// Returns all file paths related to this recipe.
    ///
    /// Includes:
    /// - Title image (if any)
    /// - Step/section images
    /// - Referenced recipe .cook files (detected via `@./path` syntax)
    /// - Recursively: related files of referenced recipes
    ///
    /// Returns an empty Vec for content-based recipes.
    /// Missing referenced files are silently skipped.
    /// Cycles are detected and broken automatically.
    pub fn related_files(&self) -> Vec<Utf8PathBuf> {
        let path = match &self.source {
            RecipeSource::Path { path } => path,
            RecipeSource::Content { .. } => return Vec::new(),
        };
        let mut visited = std::collections::HashSet::new();
        let mut result = Vec::new();
        collect_related_files(path, &mut visited, &mut result);
        result
    }
```

**Step 2: Add `collect_related_files` helper**

Add this function after `extract_recipe_references` (before `#[cfg(test)]`):

```rust
/// Recursively collects all files related to a recipe.
///
/// Adds image paths and referenced recipe paths to `result`.
/// Uses `visited` to prevent cycles and deduplication.
fn collect_related_files(
    recipe_path: &Utf8Path,
    visited: &mut std::collections::HashSet<Utf8PathBuf>,
    result: &mut Vec<Utf8PathBuf>,
) {
    // Mark as visited to prevent cycles
    if !visited.insert(recipe_path.to_path_buf()) {
        return;
    }

    // Collect title image
    if let Some(image_path) = find_title_image(recipe_path) {
        result.push(image_path);
    }

    // Collect step images
    let step_images = find_step_images(recipe_path);
    for steps in step_images.images.values() {
        for image_path in steps.values() {
            result.push(Utf8PathBuf::from(image_path));
        }
    }

    // Read content and extract recipe references
    let content = match std::fs::read_to_string(recipe_path) {
        Ok(c) => c,
        Err(_) => return,
    };

    let dir = recipe_path.parent().unwrap_or(recipe_path);
    for ref_path_str in extract_recipe_references(&content) {
        // Resolve relative to recipe's directory
        let ref_path = dir.join(&ref_path_str);

        // Try with .cook extension if no extension present
        let candidates = if ref_path.extension().is_some() {
            vec![ref_path]
        } else {
            vec![ref_path.with_extension("cook")]
        };

        for candidate in candidates {
            if candidate.exists() && !visited.contains(&candidate) {
                result.push(candidate.clone());
                collect_related_files(&candidate, visited, result);
            }
        }
    }
}
```

**Step 3: Run tests to verify they pass**

Run: `cargo test --lib model::recipe_entry::tests::test_related_files`
Expected: All 4 tests PASS

**Step 4: Commit**

```bash
git add src/model/recipe_entry.rs
git commit -m "feat: add RecipeEntry::related_files() with image collection"
```

---

### Task 6: Write tests for recursive recipe references

**Files:**
- Modify: `src/model/recipe_entry.rs` (add tests)

**Step 1: Write tests for recipe reference following**

Add these tests at the end of `mod tests`:

```rust
    #[test]
    fn test_related_files_with_referenced_recipe() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // Create a subdirectory for the referenced recipe
        let sauces_dir = temp_dir_path.join("sauces");
        std::fs::create_dir_all(&sauces_dir).unwrap();

        // Create the referenced recipe with its own image
        create_test_recipe(&sauces_dir, "Hollandaise", "Melt @butter{100%g}");
        create_test_image(&sauces_dir, "Hollandaise", "jpg");

        // Create the main recipe that references it
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "Eggs Benedict",
            "Pour @./sauces/Hollandaise{150%g} over eggs.",
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Should include: sauces/Hollandaise.cook + sauces/Hollandaise.jpg
        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.as_str().ends_with("Hollandaise.cook")));
        assert!(files.iter().any(|f| f.as_str().ends_with("Hollandaise.jpg")));
    }

    #[test]
    fn test_related_files_recursive() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let base_dir = temp_dir_path.join("base");
        std::fs::create_dir_all(&base_dir).unwrap();

        let sauces_dir = temp_dir_path.join("sauces");
        std::fs::create_dir_all(&sauces_dir).unwrap();

        // base/Stock.cook (leaf - no references)
        create_test_recipe(&base_dir, "Stock", "Simmer @bones{500%g}");
        create_test_image(&base_dir, "Stock", "png");

        // sauces/Gravy.cook -> references base/Stock
        create_test_recipe(
            &sauces_dir,
            "Gravy",
            "Add @../base/Stock{200%ml} and thicken.",
        );

        // Main recipe -> references sauces/Gravy
        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "Roast Dinner",
            "Serve with @./sauces/Gravy{100%ml}.",
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Should include:
        // - sauces/Gravy.cook (direct reference)
        // - base/Stock.cook (transitive reference from Gravy)
        // - base/Stock.png (image of Stock)
        assert_eq!(files.len(), 3);
        assert!(files.iter().any(|f| f.as_str().ends_with("Gravy.cook")));
        assert!(files.iter().any(|f| f.as_str().ends_with("Stock.cook")));
        assert!(files.iter().any(|f| f.as_str().ends_with("Stock.png")));
    }

    #[test]
    fn test_related_files_circular_reference() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        // Recipe A references Recipe B, Recipe B references Recipe A
        create_test_recipe(
            &temp_dir_path,
            "RecipeA",
            "Use @./RecipeB{100%g} as base.",
        );
        create_test_recipe(
            &temp_dir_path,
            "RecipeB",
            "Use @./RecipeA{50%g} as topping.",
        );

        let recipe_path = temp_dir_path.join("RecipeA.cook");
        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Should include RecipeB.cook but not loop infinitely
        assert_eq!(files.len(), 1);
        assert!(files.iter().any(|f| f.as_str().ends_with("RecipeB.cook")));
    }

    #[test]
    fn test_related_files_missing_reference() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();

        let recipe_path = create_test_recipe(
            &temp_dir_path,
            "incomplete",
            "Use @./nonexistent/Recipe{100%g}.",
        );

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let files = recipe.related_files();

        // Missing references are silently skipped
        assert!(files.is_empty());
    }
```

**Step 2: Run tests to verify they pass**

Run: `cargo test --lib model::recipe_entry::tests::test_related_files`
Expected: All 8 tests (4 from Task 4 + 4 new) PASS

**Step 3: Commit**

```bash
git add src/model/recipe_entry.rs
git commit -m "test: add tests for recursive recipe reference following and edge cases"
```

---

### Task 7: Add FFI binding for `related_files`

**Files:**
- Modify: `src/ffi.rs:194-259` (add method to `FfiRecipeEntry` impl block)

**Step 1: Write test first**

Add this test at the end of `mod tests` in `src/ffi.rs`:

```rust
    #[test]
    fn test_related_files_ffi() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();

        let sauces_dir = format!("{}/sauces", temp_path);
        fs::create_dir_all(&sauces_dir).unwrap();

        // Referenced recipe with image
        create_test_recipe(&sauces_dir, "Hollandaise", "Melt @butter{100%g}");
        fs::write(format!("{}/Hollandaise.jpg", sauces_dir), b"").unwrap();

        // Main recipe
        let path = create_test_recipe(
            temp_path,
            "EggsBenedict",
            "Pour @./sauces/Hollandaise{150%g} over eggs.",
        );

        let recipe = recipe_from_path(path).unwrap();
        let files = recipe.related_files();

        assert_eq!(files.len(), 2);
        assert!(files.iter().any(|f| f.ends_with("Hollandaise.cook")));
        assert!(files.iter().any(|f| f.ends_with("Hollandaise.jpg")));
    }
```

**Step 2: Add `related_files` to `FfiRecipeEntry` impl**

Add this method inside the `#[uniffi::export] impl FfiRecipeEntry` block (after the `get_metadata_value` method):

```rust
    /// Returns all file paths related to this recipe.
    ///
    /// Includes images, referenced recipe files, and recursively
    /// related files of referenced recipes.
    pub fn related_files(&self) -> Vec<String> {
        self.inner
            .related_files()
            .into_iter()
            .map(|p| p.to_string())
            .collect()
    }
```

**Step 3: Run tests**

Run: `cargo test --lib ffi::tests::test_related_files_ffi`
Expected: PASS

**Step 4: Run full test suite**

Run: `cargo test`
Expected: All tests PASS

**Step 5: Commit**

```bash
git add src/ffi.rs
git commit -m "feat: expose related_files via FFI bindings"
```

---

### Task 8: Final verification

**Step 1: Run all tests**

Run: `cargo test`
Expected: All tests PASS

**Step 2: Run clippy**

Run: `cargo clippy -- -D warnings`
Expected: No warnings

**Step 3: Check formatting**

Run: `cargo fmt -- --check`
Expected: No formatting issues

**Step 4: Verify it builds for release**

Run: `cargo build --release`
Expected: Builds successfully
