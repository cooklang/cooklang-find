# list_menus_for_date Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Add a function that returns the `.menu` files whose section headers contain a given date string, plus a uniffi FFI wrapper.

**Architecture:** A new `src/menu/` module globs `**/*.menu` under one or more base directories, loads each as a `RecipeEntry`, scans its raw content line-by-line for a section header (`=`-prefixed line) whose name contains the date substring, and returns matching entries. Unreadable files are skipped (mirroring `build_tree`). The library does no date parsing; the caller supplies the literal date string and computes its own today/tomorrow.

**Tech Stack:** Rust, `camino` (Utf8Path), `glob`, `thiserror`, `uniffi`; tests use `tempfile`.

---

## Spec

See `docs/superpowers/specs/2026-06-24-list-menus-for-date-design.md`.

## File Structure

- **Create:** `src/menu/mod.rs` — the `menu` module: `MenuError`, the private `section_header_contains_date` helper, and the public `list_menus_for_date` function, with unit + integration tests.
- **Modify:** `src/lib.rs` — register `pub mod menu;` and re-export `list_menus_for_date`.
- **Modify:** `src/ffi.rs` — add `From<MenuError>` (+ a `CooklangError::MenuError` variant) and the `#[uniffi::export] list_menus_for_date` wrapper.

### Design note (deviation from spec)

The spec lists a "wrapped `RecipeEntryError`" variant on `MenuError`. Because individual unreadable/unparseable files are **skipped** (`Err(_) => continue`, exactly like `build_tree` in `src/tree/mod.rs:97-100`), no `RecipeEntryError` is ever propagated from this module, so that variant would be dead code. `MenuError` therefore carries only `GlobError`, `PatternError`, and `IoError` (the last used for the invalid-UTF-8 path case, which *is* propagated). This matches `TreeError`, which also does not wrap `RecipeEntryError`.

---

## Task 1: Menu module scaffold — `MenuError` + section-header helper

**Files:**
- Create: `src/menu/mod.rs`
- Modify: `src/lib.rs` (register module)
- Test: inline `#[cfg(test)]` module in `src/menu/mod.rs`

- [ ] **Step 1: Register the module in `src/lib.rs`**

Add the module declaration. Insert after the `pub mod fetcher;` block (around `src/lib.rs:40`), keeping modules alphabetical-ish with the existing order:

```rust
/// Menu discovery by date (sections containing a date string).
pub mod menu;
```

(Leave the `pub use` re-export for Task 2 — `list_menus_for_date` does not exist yet.)

- [ ] **Step 2: Create `src/menu/mod.rs` with the error type and helper**

```rust
//! Menu discovery by date.
//!
//! This module finds Cooklang `.menu` files that are "active" on a given date.
//! A menu file is active for a date when one of its section headers contains
//! that date string (for example, a section `= 2026-06-24 Dinner` makes the
//! file active for `2026-06-24`).
//!
//! The library performs no date parsing or timezone handling: the caller
//! supplies the date as an opaque string that is matched literally.

use crate::model::RecipeEntry;
use camino::{Utf8Path, Utf8PathBuf};
use std::collections::HashSet;
use thiserror::Error;

/// Errors that can occur while listing menus by date.
#[derive(Error, Debug)]
pub enum MenuError {
    #[error("Failed to read directory: {0}")]
    GlobError(#[from] glob::GlobError),

    #[error("Failed to create glob pattern: {0}")]
    PatternError(#[from] glob::PatternError),

    #[error("Failed to read file: {0}")]
    IoError(#[from] std::io::Error),
}

/// Returns true if `line` is a Cooklang section header whose name contains `date`.
///
/// A section header is a line whose trimmed form starts with `=`. The section
/// name is obtained by stripping leading/trailing `=` characters and surrounding
/// whitespace (handles both `= Name` and `== Name ==` forms). The `date` is
/// matched as a literal substring of that name.
fn section_header_contains_date(line: &str, date: &str) -> bool {
    let trimmed = line.trim();
    if !trimmed.starts_with('=') {
        return false;
    }
    let name = trimmed.trim_matches('=').trim();
    name.contains(date)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_exact_date_section() {
        assert!(section_header_contains_date("= 2026-06-24", "2026-06-24"));
    }

    #[test]
    fn matches_date_with_label() {
        assert!(section_header_contains_date(
            "= 2026-06-24 Dinner",
            "2026-06-24"
        ));
    }

    #[test]
    fn matches_double_equals_form() {
        assert!(section_header_contains_date(
            "== 2026-06-24 Dinner ==",
            "2026-06-24"
        ));
    }

    #[test]
    fn ignores_non_header_line() {
        assert!(!section_header_contains_date(
            "Cook the eggs on 2026-06-24",
            "2026-06-24"
        ));
    }

    #[test]
    fn ignores_header_without_date() {
        assert!(!section_header_contains_date("= Breakfast", "2026-06-24"));
    }
}
```

- [ ] **Step 3: Run the helper tests to verify they pass**

Run: `cargo test --lib menu::tests`
Expected: PASS (5 tests in `menu::tests`).

- [ ] **Step 4: Commit**

```bash
git add src/menu/mod.rs src/lib.rs
git commit -m "feat: add menu module with section-header date matching"
```

---

## Task 2: `list_menus_for_date` core function

**Files:**
- Modify: `src/menu/mod.rs` (add function + integration tests)
- Modify: `src/lib.rs` (re-export the function)
- Test: inline `#[cfg(test)]` module in `src/menu/mod.rs`

- [ ] **Step 1: Write the failing integration tests**

Append these tests inside the existing `mod tests` block in `src/menu/mod.rs` (after the helper tests, before the closing `}`). Also add the imports the new tests need at the top of `mod tests`, right under `use super::*;`:

```rust
    use std::fs;
    use tempfile::TempDir;

    /// Writes a file with the given name (including extension) into `dir`.
    fn write_file(dir: &Utf8Path, name: &str, content: &str) {
        fs::write(dir.join(name), content).unwrap();
    }

    fn temp_dir() -> (TempDir, Utf8PathBuf) {
        let temp = TempDir::new().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        (temp, path)
    }

    #[test]
    fn includes_menu_with_matching_date_section() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= 2026-06-24\n\nServe @pancakes{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn includes_menu_with_dated_label_section() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= 2026-06-24 Dinner\n\nServe @stew{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn excludes_menu_without_matching_date() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= 2026-06-25\n\nServe @soup{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn excludes_cook_files_even_when_date_present() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "recipe.cook", "= 2026-06-24\n\nServe @cake{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn excludes_date_only_in_body() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= Dinner\n\nMade on 2026-06-24 with @eggs{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert!(results.is_empty());
    }

    #[test]
    fn merges_results_across_base_dirs() {
        let (_t1, dir1) = temp_dir();
        let (_t2, dir2) = temp_dir();
        write_file(&dir1, "a.menu", "= 2026-06-24\n\n@a{}\n");
        write_file(&dir2, "b.menu", "= 2026-06-24\n\n@b{}\n");

        let results = list_menus_for_date(&[&dir1, &dir2], "2026-06-24").unwrap();

        assert_eq!(results.len(), 2);
    }

    #[test]
    fn returns_empty_when_no_menus_match() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= Breakfast\n\n@toast{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert!(results.is_empty());
    }
```

- [ ] **Step 2: Run the tests to verify they fail**

Run: `cargo test --lib menu::tests`
Expected: FAIL to compile with "cannot find function `list_menus_for_date` in this scope".

- [ ] **Step 3: Implement `list_menus_for_date`**

Add this function in `src/menu/mod.rs`, after `section_header_contains_date` and before the `#[cfg(test)]` module:

```rust
/// Returns all `.menu` files under `base_dirs` that have a section header
/// containing `date`.
///
/// Only `.menu` files are scanned; `.cook` recipe files are ignored. A file is
/// included if **any** of its section headers contains the `date` substring.
/// Dates appearing only in step/body lines do not cause a match. Files that
/// cannot be read or parsed are silently skipped. The same path is never
/// returned twice, even if base directories overlap.
///
/// `date` is matched literally; this function does no date parsing or
/// validation. The caller is responsible for computing the date string
/// (for example, the host application's local "today" or "tomorrow").
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::list_menus_for_date;
/// use camino::Utf8Path;
///
/// let menus = list_menus_for_date(&[Utf8Path::new("./menus")], "2026-06-24")?;
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn list_menus_for_date<P: AsRef<Utf8Path>>(
    base_dirs: &[P],
    date: &str,
) -> Result<Vec<RecipeEntry>, MenuError> {
    let mut seen: HashSet<Utf8PathBuf> = HashSet::new();
    let mut menus = Vec::new();

    for base_dir in base_dirs {
        let pattern = base_dir.as_ref().join("**/*.menu").to_string();
        for entry in glob::glob(&pattern)? {
            let path = entry?;
            let path = Utf8PathBuf::from_path_buf(path).map_err(|_| {
                MenuError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Path contains invalid UTF-8",
                ))
            })?;

            if !seen.insert(path.clone()) {
                continue;
            }

            let recipe = match RecipeEntry::from_path(path.clone()) {
                Ok(r) => r,
                Err(_) => continue, // Skip files whose content isn't available
            };

            let content = match recipe.content() {
                Ok(c) => c,
                Err(_) => continue,
            };

            if content
                .lines()
                .any(|line| section_header_contains_date(line, date))
            {
                menus.push(recipe);
            }
        }
    }

    Ok(menus)
}
```

- [ ] **Step 4: Re-export from `src/lib.rs`**

Add the re-export next to the other `pub use` lines (after `pub use fetcher::{get_recipe, get_recipe_str};` near `src/lib.rs:54`):

```rust
pub use menu::list_menus_for_date;
```

- [ ] **Step 5: Run the tests to verify they pass**

Run: `cargo test --lib menu::tests`
Expected: PASS (all `menu::tests`, helper + integration).

- [ ] **Step 6: Commit**

```bash
git add src/menu/mod.rs src/lib.rs
git commit -m "feat: add list_menus_for_date"
```

---

## Task 3: FFI wrapper

**Files:**
- Modify: `src/ffi.rs`

- [ ] **Step 1: Add the `MenuError` import and a `CooklangError::MenuError` variant**

In `src/ffi.rs`, extend the `use crate::menu::...` imports. Add this line after the existing `use crate::search::{...};` line (around `src/ffi.rs:8`):

```rust
use crate::menu::{list_menus_for_date as list_menus_for_date_internal, MenuError};
```

Then extend the `camino` import (currently `use camino::Utf8Path;` at `src/ffi.rs:10`) to also bring in `Utf8PathBuf`:

```rust
use camino::{Utf8Path, Utf8PathBuf};
```

Add a variant to the `CooklangError` enum (after the `TreeError { reason: String }` variant, `src/ffi.rs:27`):

```rust
    /// Menu listing operation failed
    MenuError { reason: String },
```

Add the matching `Display` arm (after the `TreeError` arm, `src/ffi.rs:38`):

```rust
            CooklangError::MenuError { reason } => write!(f, "Menu error: {}", reason),
```

- [ ] **Step 2: Add the `From<MenuError>` conversion**

Add after the `impl From<TreeError> for CooklangError` block (`src/ffi.rs:82-88`):

```rust
impl From<MenuError> for CooklangError {
    fn from(e: MenuError) -> Self {
        CooklangError::MenuError {
            reason: e.to_string(),
        }
    }
}
```

- [ ] **Step 3: Add the exported FFI function**

Add after the `search` FFI function (after the closing `}` at `src/ffi.rs:445`):

```rust
/// Lists menu files that have a section header containing the given date.
///
/// Only `.menu` files are scanned; a file is included if any of its section
/// headers contains the `date` substring. The date is matched literally — the
/// caller supplies it (for example, the host app's local "today" or "tomorrow").
///
/// # Arguments
/// * `base_dirs` - Root directories to scan
/// * `date` - The date string to match (e.g. "2026-06-24")
///
/// # Returns
/// List of matching menu recipes.
#[uniffi::export]
pub fn list_menus_for_date(
    base_dirs: Vec<String>,
    date: String,
) -> Result<Vec<Arc<FfiRecipeEntry>>, CooklangError> {
    let dirs: Vec<Utf8PathBuf> = base_dirs.into_iter().map(Utf8PathBuf::from).collect();
    let results = list_menus_for_date_internal(&dirs, &date)?;
    Ok(results
        .into_iter()
        .map(|r| Arc::new(FfiRecipeEntry::new(r)))
        .collect())
}
```

- [ ] **Step 4: Build and run the full test suite**

Run: `cargo test`
Expected: PASS (existing tests plus the new `menu::tests`), no warnings about unused imports/variants.

- [ ] **Step 5: Verify it compiles cleanly**

Run: `cargo build`
Expected: Builds with no errors or warnings.

- [ ] **Step 6: Commit**

```bash
git add src/ffi.rs
git commit -m "feat: add list_menus_for_date FFI wrapper"
```

---

## Self-Review Notes

- **Spec coverage:** API (Task 2), multi-dir support (Task 2 `base_dirs: &[P]`), FFI wrapper (Task 3), new module (Task 1/2), section-contains-date matching (Task 1 helper), `.menu`-only scanning + body-line exclusion + dedup (Task 2 tests + impl), skip-unreadable (Task 2 impl `Err(_) => continue`), error type (Task 1, minus the skipped `RecipeEntryError` variant — see Design note). All covered.
- **Type consistency:** `list_menus_for_date` signature is identical in lib re-export and FFI internal alias; `MenuError` variants used in `From` impl all exist; `FfiRecipeEntry::new` matches existing usage in the `search` wrapper.
- **No placeholders:** every code/test step shows complete code.
