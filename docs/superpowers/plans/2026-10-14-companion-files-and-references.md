# Companion Files and Recipe References Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make cooklang-find the one place that knows which files belong to a recipe and which file a `@./path` reference means, and fix step-image section numbering across the FFI.

**Architecture:** A new private `src/reference.rs` holds the component scanner (moved from the menu parser), reference extraction and `resolve_reference`. A new `src/model/companions.rs` does one directory listing per recipe and classifies `Name.ext` / `Name.N.ext` / `Name.S.N.ext`; `title_image()`, `step_images()`, `companion_files()` and `renamed_companions()` are all views over that listing. `related_files` and menu scaling both resolve references from the collection root through `resolve_reference`. The FFI mirrors each new function.

**Tech Stack:** Rust, `camino`, `serde`, `uniffi` 0.32; tests use `tempfile`, `indoc`. `make lint` runs `cargo fmt --check` and `cargo clippy -- -D warnings`.

---

## Spec

`docs/superpowers/specs/2026-10-14-companion-files-and-references-design.md` is authoritative. One refinement made while planning (Task 3 updates the spec text): when a reference ends in `.cook`/`.menu` it is tried as written **and then** with `.cook`/`.menu` appended, because `Utf8Path::extension()` of `Dr.Cook` is `Cook`, and the spec requires `./Dr.Cook` → `Dr.Cook.cook`.

## File Structure

- **Create:** `src/reference.rs` — `Component`, `component()`, `extract_references()`, `resolve_reference()`.
- **Create:** `src/model/companions.rs` — `IMAGE_EXTENSIONS`, `Slot`, `scan()`, `companion_files()`, `RenamedFile`, `renamed_companions()`.
- **Modify:** `src/model/recipe_entry.rs` — `StepImageCollection` gets `linear`/`sectioned`; `RecipeEntry` caches the companion scan; `companion_files()`, `related_files(base_dir)`; old `find_title_image`/`find_step_images`/`parse_image_numbers`/`extract_recipe_references` removed with their `glob`/`regex` imports.
- **Modify:** `src/model/mod.rs` — declare `companions`, re-export the new items.
- **Modify:** `src/menu/parse.rs` — `parse_component` uses `crate::reference::component`; `braced`/`single_word_len` deleted here.
- **Modify:** `src/menu/scale.rs` — `RecipeSize::load` uses `resolve_reference`.
- **Modify:** `src/lib.rs` — `mod reference;`, `pub use reference::resolve_reference;`, crate doc.
- **Modify:** `src/ffi.rs` — `FfiStepImages` conversion, `FfiRenamedFile`, free functions `companion_files`, `renamed_companions`, `resolve_reference`, `FfiRecipeEntry::companion_files`, `related_files(base_dir)`.
- **Modify:** `README.md`, `BINDINGS.md`, the spec (Task 3 note).

Run `cargo test` for everything; `cargo test --lib reference::` / `model::` / `menu::` / `ffi::` to narrow.

---

## Task 1: Move the component scanner into `src/reference.rs`

**Files:**
- Create: `src/reference.rs`
- Modify: `src/lib.rs:59` (after `mod walk;`)
- Modify: `src/menu/parse.rs:260-327`

- [ ] **Step 1: Create `src/reference.rs` with the scanner and its tests**

```rust
//! Recipe references (`@./path`), shared by `.cook` and `.menu` handling:
//! the component scanner, reference extraction and reference resolution.
//!
//! The scanner is the menu parser's rule, used for recipe content too so a
//! `@./path` on a line means the same file in either kind of file. The
//! single-word fallback is deliberately more lenient than cooklang-rs, whose
//! lexer breaks a word at `.` and `/` and so never accepts a braceless
//! `@./Pesto`; do not "fix" it to match the parser.

/// The component after an `@`.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct Component<'a> {
    /// The name: text before the braces, or the single word.
    pub name: &'a str,
    /// Text inside the braces, when there are braces.
    pub amount: Option<&'a str>,
    /// Bytes consumed from the body: modifier and trailing `(note)` included.
    pub len: usize,
}

/// Scans the component after an `@`, or `None` when the text after `@` is
/// not one (a stray `@` before a comment, or nothing word-like).
///
/// - A leading modifier (`?`, `+`, `-`, `&`) is skipped.
/// - Braced form: the name is everything before the first `{` (with a `}`
///   after it), trailing whitespace trimmed; a `(note)` right after the
///   braces is consumed. The text before `{` must not be empty, start with
///   whitespace, or contain `@`, `#`, `~`, `}` or `--`; otherwise the
///   single-word rule applies.
/// - Single word: up to the first whitespace or any of `,;:!?(){}@`, then
///   trailing dots are trimmed.
pub(crate) fn component(body: &str) -> Option<Component<'_>> {
    // `@-- note` is a stray `@` before a comment, not an `@-` modifier.
    if body.starts_with("--") {
        return None;
    }
    let rest = body.strip_prefix(['?', '+', '-', '&']).unwrap_or(body);
    let modifiers = body.len() - rest.len();

    let (name, amount, len) = match braced(rest) {
        Some((name, amount, mut len)) => {
            if rest[len..].starts_with('(') {
                if let Some(close) = rest[len..].find(')') {
                    len += close + 1;
                }
            }
            (name, Some(amount), len)
        }
        None => {
            let len = single_word_len(rest);
            if len == 0 {
                return None;
            }
            (&rest[..len], None, len)
        }
    };
    Some(Component {
        name,
        amount,
        len: modifiers + len,
    })
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
        .find(|c: char| c.is_whitespace() || ",;:!?(){}@".contains(c))
        .unwrap_or(body.len());
    body[..end].trim_end_matches('.').len()
}

/// Whether a component name is a recipe reference.
pub(crate) fn is_reference(name: &str) -> bool {
    name.starts_with("./") || name.starts_with("../")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn name(body: &str) -> Option<&str> {
        component(body).map(|c| c.name)
    }

    #[test]
    fn braced_name_runs_to_the_brace() {
        assert_eq!(
            component("./Pesto Sauce{100%g} and stir"),
            Some(Component {
                name: "./Pesto Sauce",
                amount: Some("100%g"),
                len: "./Pesto Sauce{100%g}".len()
            })
        );
    }

    #[test]
    fn bare_name_is_one_word_and_keeps_inner_dots() {
        assert_eq!(name("./Dr.Cook and more"), Some("./Dr.Cook"));
        assert_eq!(name("./Sunday.menu"), Some("./Sunday.menu"));
        assert_eq!(name("./Pesto Sauce"), Some("./Pesto"));
    }

    #[test]
    fn trailing_dot_is_punctuation() {
        assert_eq!(name("./Pesto."), Some("./Pesto"));
    }

    #[test]
    fn another_component_before_the_brace_forces_single_word() {
        assert_eq!(name("./Pesto Sauce and @salt{}"), Some("./Pesto"));
    }

    #[test]
    fn unclosed_brace_falls_back_to_single_word() {
        assert_eq!(name("./Pesto{2"), Some("./Pesto"));
    }

    #[test]
    fn modifier_is_skipped_and_counted() {
        let c = component("?./Pesto{}").unwrap();
        assert_eq!(c.name, "./Pesto");
        assert_eq!(c.len, "?./Pesto{}".len());
    }

    #[test]
    fn note_after_braces_is_consumed() {
        let c = component("./Pesto{}(cooked) then").unwrap();
        assert_eq!(c.len, "./Pesto{}(cooked)".len());
    }

    #[test]
    fn comment_marker_is_not_a_component() {
        assert_eq!(component("-- note"), None);
        assert_eq!(component(" noon"), None);
    }
}
```

- [ ] **Step 2: Declare the module in `src/lib.rs`**

After `mod walk;` add:

```rust
mod reference;
```

- [ ] **Step 3: Run the new tests**

Run: `cargo test --lib reference::`
Expected: 8 passed.

- [ ] **Step 4: Point the menu parser at the shared scanner**

In `src/menu/parse.rs`, replace `parse_component`, `braced` and `single_word_len` (lines 260-327) with:

```rust
/// Parses the component after an `@`, returning it and the bytes consumed.
fn parse_component(s: &str) -> Option<(MenuItem, usize)> {
    let Component { name, amount, len } = component(s)?;
    let (quantity, unit) = split_amount(amount);
    let item = if is_reference(name) {
        recipe_reference(name, quantity, unit)?
    } else if name.is_empty() {
        return None;
    } else {
        MenuItem::Ingredient {
            name: name.to_string(),
            quantity,
            unit,
        }
    };
    Some((item, len))
}
```

and add to the imports at the top of the file:

```rust
use crate::reference::{component, is_reference, Component};
```

- [ ] **Step 5: Run the menu tests unchanged**

Run: `cargo test --lib menu::`
Expected: all pass, including `unbraced_recipe_reference`, `unbraced_ingredient_drops_trailing_period`, `menu_reference_keeps_menu_suffix`, `unclosed_brace_falls_back_to_single_word`, `at_followed_by_comment_is_not_an_ingredient`, `modifiers_are_ignored`, `trailing_note_in_parentheses_is_skipped`.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt && make lint`
Expected: clean (`cargo fmt` first, because several snippets here are not rustfmt-clean as pasted).

```bash
git add src/reference.rs src/lib.rs src/menu/parse.rs
git commit -m "refactor: share the component scanner between menus and recipes"
```

---

## Task 2: `extract_references` in `src/reference.rs`

**Files:**
- Modify: `src/reference.rs`

- [ ] **Step 1: Add failing tests**

Append inside `mod tests`:

```rust
    #[test]
    fn extracts_braced_and_bare_references_in_order() {
        let refs = extract_references(
            "Serve @./sauces/Hollandaise{150%g} with @./sides/Asparagus{200%g}.\nThen @../base/Stock.",
        );
        assert_eq!(
            refs,
            vec!["./sauces/Hollandaise", "./sides/Asparagus", "../base/Stock"]
        );
    }

    #[test]
    fn ignores_plain_ingredients_and_deduplicates() {
        let refs = extract_references(
            "Add @salt{1%tsp}. Use @./base/Stock{100%ml} twice and @./base/Stock{200%ml} again.",
        );
        assert_eq!(refs, vec!["./base/Stock"]);
    }

    #[test]
    fn multi_word_and_dotted_names() {
        assert_eq!(
            extract_references("@./Pesto Sauce{100%g} @./Dr.Cook{} @./Sunday.menu"),
            vec!["./Pesto Sauce", "./Dr.Cook", "./Sunday.menu"]
        );
    }

    #[test]
    fn scans_line_by_line() {
        // A brace on the next line never joins the name on this one.
        assert_eq!(
            extract_references("@./Pesto\nthen {stir}"),
            vec!["./Pesto"]
        );
        assert_eq!(
            extract_references("Add @./Pesto Sauce and @salt{}"),
            vec!["./Pesto"]
        );
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            extract_references("@./Pesto{} -- was @./Old{}\n@-- not a modifier"),
            vec!["./Pesto"]
        );
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test --lib reference::`
Expected: compile error, `extract_references` not found.

- [ ] **Step 3: Implement**

Add above `mod tests`:

```rust
use std::collections::HashSet;

/// The recipe references in `content`, in order of first appearance,
/// deduplicated, as written (`./sauces/Hollandaise`, `../Pesto`).
///
/// Scans line by line with [`component`], so a name never spans lines, and
/// stops a line at `--` (a comment).
#[allow(dead_code)] // wired into related_files in a later commit
pub(crate) fn extract_references(content: &str) -> Vec<String> {
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    for line in content.lines() {
        let mut i = 0;
        while i < line.len() {
            let rest = &line[i..];
            if rest.starts_with("--") {
                break;
            }
            if let Some(body) = rest.strip_prefix('@') {
                if let Some(c) = component(body) {
                    if is_reference(c.name) && seen.insert(c.name.to_string()) {
                        refs.push(c.name.to_string());
                    }
                    i += 1 + c.len;
                    continue;
                }
            }
            i += rest.chars().next().map_or(1, char::len_utf8);
        }
    }
    refs
}
```

- [ ] **Step 4: Run the tests**

Run: `cargo test --lib reference::`
Expected: 13 passed.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && make lint`
Expected: clean. The `#[allow(dead_code)]` is what keeps `-D warnings` quiet until Task 5 Step 6 uses the function; that step removes the attribute.

```bash
git add src/reference.rs
git commit -m "feat: extract recipe references with the shared scanner"
```

---

## Task 3: `resolve_reference`

**Files:**
- Modify: `src/reference.rs`
- Modify: `src/lib.rs` (re-export)
- Modify: `docs/superpowers/specs/2026-10-14-companion-files-and-references-design.md` §3 step 3

- [ ] **Step 1: Add failing tests**

Append inside `mod tests`:

```rust
    use camino::{Utf8Path, Utf8PathBuf};
    use std::fs;
    use tempfile::TempDir;

    fn root() -> (TempDir, Utf8PathBuf) {
        let temp = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        (temp, dir)
    }

    fn touch(dir: &Utf8Path, name: &str) -> Utf8PathBuf {
        let path = dir.join(name);
        fs::create_dir_all(path.parent().unwrap()).unwrap();
        fs::write(&path, "").unwrap();
        path
    }

    #[test]
    fn resolves_cook_then_menu_from_the_root() {
        let (_t, dir) = root();
        let pesto = touch(&dir, "sauces/Pesto.cook");
        let sunday = touch(&dir, "Sunday.menu");
        assert_eq!(resolve_reference(&dir, "./sauces/Pesto"), Some(pesto.clone()));
        assert_eq!(resolve_reference(&dir, "sauces/Pesto"), Some(pesto));
        assert_eq!(resolve_reference(&dir, "./Sunday"), Some(sunday.clone()));
        assert_eq!(resolve_reference(&dir, "./Sunday.menu"), Some(sunday.clone()));
        assert_eq!(resolve_reference(&dir, "./Sunday.MENU"), Some(sunday));
    }

    #[test]
    fn cook_wins_over_menu() {
        let (_t, dir) = root();
        let cook = touch(&dir, "Pancakes.cook");
        touch(&dir, "Pancakes.menu");
        assert_eq!(resolve_reference(&dir, "./Pancakes"), Some(cook));
    }

    #[test]
    fn dotted_names_and_explicit_cook_suffix() {
        let (_t, dir) = root();
        let dr = touch(&dir, "Dr.Cook.cook");
        let pesto = touch(&dir, "Pesto.cook");
        let stew = touch(&dir, "Mr. Smith's Stew.cook");
        assert_eq!(resolve_reference(&dir, "./Dr.Cook"), Some(dr));
        assert_eq!(resolve_reference(&dir, "./Pesto.cook"), Some(pesto.clone()));
        assert_eq!(resolve_reference(&dir, "./Pesto.COOK"), Some(pesto));
        assert_eq!(resolve_reference(&dir, "./Mr. Smith's Stew"), Some(stew));
    }

    #[test]
    fn extension_case_of_the_file_does_not_matter() {
        let (_t, dir) = root();
        let upper = touch(&dir, "Pesto.COOK");
        assert_eq!(resolve_reference(&dir, "./Pesto"), Some(upper));
    }

    #[test]
    fn dot_dot_is_normalised_inside_the_root_and_refused_outside() {
        let (_t, dir) = root();
        let b = touch(&dir, "b.cook");
        touch(&dir, "a/.keep");
        assert_eq!(resolve_reference(&dir, "./a/../b"), Some(b));
        assert_eq!(resolve_reference(&dir, "../b"), None);
        assert_eq!(resolve_reference(&dir, "./../../b"), None);
    }

    #[test]
    fn folders_and_missing_files_are_not_matches() {
        let (_t, dir) = root();
        touch(&dir, "Pesto/.keep");
        assert_eq!(resolve_reference(&dir, "./Pesto"), None);
        assert_eq!(resolve_reference(&dir, "./Nothing"), None);
        assert_eq!(resolve_reference(&dir, "./"), None);
        assert_eq!(resolve_reference(&dir, ""), None);
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test --lib reference::`
Expected: compile error, `resolve_reference` not found.

- [ ] **Step 3: Implement**

Add above `mod tests`:

```rust
use crate::walk::has_extension;
use camino::{Utf8Path, Utf8PathBuf};

/// Resolves a recipe reference (`./sauces/Hollandaise`, `../Pesto`,
/// `Sunday.menu`) against the collection root `base_dir`.
///
/// Segments `.` and empty are dropped and `..` pops the previous segment;
/// a reference that would leave the root is `None`. A last segment ending
/// in `.cook` or `.menu` (any ASCII case) is tried as written first; then,
/// for every reference, `<name>.cook` and `<name>.menu` are tried, so
/// `./Dr.Cook` still finds `Dr.Cook.cook`. The folder is listed once and
/// the extension is compared ignoring ASCII case, so `./Pesto` finds
/// `Pesto.COOK` on any filesystem; the stem must match exactly. The first
/// existing regular file wins, in its real spelling; `None` otherwise.
/// Symlinks are not followed for the containment check.
///
/// This is the one rule for references, used by `RecipeEntry::related_files`
/// and `Menu::resolve_scales`, and the same rule CookCLI applies.
pub fn resolve_reference(base_dir: &Utf8Path, reference: &str) -> Option<Utf8PathBuf> {
    let mut segments: Vec<&str> = Vec::new();
    for segment in reference.split(['/', '\\']) {
        match segment {
            "" | "." => {}
            ".." => {
                segments.pop()?;
            }
            name => segments.push(name),
        }
    }
    let name = segments.pop()?;
    let dir = segments
        .iter()
        .fold(base_dir.to_path_buf(), |dir, segment| dir.join(segment));

    let mut wanted = Vec::with_capacity(3);
    let as_path = Utf8Path::new(name);
    if has_extension(as_path, "cook") || has_extension(as_path, "menu") {
        wanted.push(name.to_string());
    }
    wanted.push(format!("{name}.cook"));
    wanted.push(format!("{name}.menu"));

    let mut names: Vec<String> = fs::read_dir(&dir)
        .ok()?
        .flatten()
        .filter_map(|entry| entry.file_name().into_string().ok())
        .collect();
    names.sort();
    wanted.iter().find_map(|want| {
        names
            .iter()
            .find(|have| same_file_name(have, want))
            .map(|have| dir.join(have))
            .filter(|path| path.is_file())
    })
}

/// Same stem, and the same extension ignoring ASCII case.
fn same_file_name(have: &str, want: &str) -> bool {
    match (have.rsplit_once('.'), want.rsplit_once('.')) {
        (Some((have_stem, have_ext)), Some((want_stem, want_ext))) => {
            have_stem == want_stem && have_ext.eq_ignore_ascii_case(want_ext)
        }
        _ => have == want,
    }
}
```

The imports at the top of the file become `use camino::{Utf8Path, Utf8PathBuf}; use std::collections::HashSet; use std::fs;` plus `use crate::walk::has_extension;` (the test module already imports `fs`, `Utf8Path`, `Utf8PathBuf` and `TempDir` on its own).

- [ ] **Step 4: Re-export from the crate root**

In `src/lib.rs`, after `pub use model::*;` add:

```rust
pub use reference::resolve_reference;
```

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib reference::`
Expected: 19 passed.

- [ ] **Step 6: Record the refinement in the spec**

In the spec, §3 step 3, replace:

```
   `menu/scale.rs` already does for `.menu`. With such a suffix, try the
   path as written. Otherwise try `<path>.cook` then `<path>.menu`. This
```

with:

```
   `menu/scale.rs` already does for `.menu`. With such a suffix, try the
   path as written first. Then, for every reference, try `<path>.cook` and
   `<path>.menu`: `Utf8Path::extension()` of `Dr.Cook` is `Cook`, so the
   fallback is what makes `./Dr.Cook` find `Dr.Cook.cook`. This
```

- [ ] **Step 7: Lint and commit**

Run: `cargo fmt && make lint`

```bash
git add src/reference.rs src/lib.rs docs/superpowers/specs/2026-10-14-companion-files-and-references-design.md
git commit -m "feat: resolve_reference, one root-relative rule for recipe references"
```

---

## Task 4: Companion scan: `src/model/companions.rs`

**Files:**
- Create: `src/model/companions.rs`
- Modify: `src/model/mod.rs`

- [ ] **Step 1: Create the module with failing tests for `scan` and `companion_files`**

```rust
//! The files that belong to a recipe by naming convention, found by one
//! listing of its folder:
//!
//! - `Name.ext` — the title image
//! - `Name.N.ext` — step N of a recipe without sections
//! - `Name.S.N.ext` — section S, step N
//!
//! `N` and `S` are one-based. `ext` is one of [`IMAGE_EXTENSIONS`], in any
//! ASCII case. `Name` is the recipe file's stem, so `Dr.Cook.cook` owns
//! `Dr.Cook.jpg`. Referenced recipes are never companions.

use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

/// Image extensions a recipe's companions may have, in preference order
/// when one slot has several files. Matched ignoring ASCII case.
pub const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp", "heic"];

/// Which companion a file is. Ordered: title, then linear steps, then
/// sectioned steps.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum Slot {
    Title,
    Linear(usize),
    Sectioned(usize, usize),
}

/// Lists the recipe's folder once and returns every companion with its
/// slot, sorted by slot, then by extension preference, then by file name.
/// Files whose names start with `.` are skipped. A folder that cannot be
/// read, or a path without a parent or stem, gives an empty list.
pub(crate) fn scan(recipe_path: &Utf8Path) -> Vec<(Slot, Utf8PathBuf)> {
    let (Some(dir), Some(stem)) = (recipe_path.parent(), recipe_path.file_stem()) else {
        return Vec::new();
    };
    let Ok(entries) = fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut found: Vec<(Slot, usize, Utf8PathBuf)> = entries
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            if name.starts_with('.') {
                return None;
            }
            let (slot, rank) = classify(&name, stem)?;
            let path = dir.join(&name);
            path.is_file().then_some((slot, rank, path))
        })
        .collect();
    found.sort();
    found
        .into_iter()
        .map(|(slot, _, path)| (slot, path))
        .collect()
}

/// `Name.ext` → `Title`, `Name.N.ext` → `Linear(N)`, `Name.S.N.ext` →
/// `Sectioned(S, N)`, with the extension's rank in [`IMAGE_EXTENSIONS`].
fn classify(file_name: &str, stem: &str) -> Option<(Slot, usize)> {
    let rest = file_name.strip_prefix(stem)?.strip_prefix('.')?;
    let (numbers, ext) = match rest.rsplit_once('.') {
        Some((numbers, ext)) => (Some(numbers), ext),
        None => (None, rest),
    };
    let rank = IMAGE_EXTENSIONS
        .iter()
        .position(|e| e.eq_ignore_ascii_case(ext))?;
    let slot = match numbers {
        None => Slot::Title,
        Some(numbers) => {
            let numbers: Vec<usize> = numbers
                .split('.')
                .map(|n| n.parse::<usize>().ok().filter(|&n| n >= 1))
                .collect::<Option<_>>()?;
            match numbers[..] {
                [step] => Slot::Linear(step),
                [section, step] => Slot::Sectioned(section, step),
                _ => return None,
            }
        }
    };
    Some((slot, rank))
}

/// The title image and every step and section image next to `recipe_path`,
/// as absolute paths: title first, then linear steps by step, then sectioned
/// images by section and step. Several files for one slot (`Name.jpg` and
/// `Name.PNG`) are all returned, in [`IMAGE_EXTENSIONS`] order. Never a
/// referenced recipe. Opens no file; see [`scan`] for what yields an empty
/// list.
pub fn companion_files(recipe_path: &Utf8Path) -> Vec<Utf8PathBuf> {
    scan(recipe_path).into_iter().map(|(_, path)| path).collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn root() -> (TempDir, Utf8PathBuf) {
        let temp = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        (temp, dir)
    }

    fn touch(dir: &Utf8Path, name: &str) -> Utf8PathBuf {
        let path = dir.join(name);
        fs::write(&path, "").unwrap();
        path
    }

    fn names(paths: &[Utf8PathBuf]) -> Vec<&str> {
        paths.iter().map(|p| p.file_name().unwrap()).collect()
    }

    #[test]
    fn classifies_title_linear_and_sectioned() {
        assert_eq!(classify("Pasta.jpg", "Pasta"), Some((Slot::Title, 0)));
        assert_eq!(classify("Pasta.3.PNG", "Pasta"), Some((Slot::Linear(3), 2)));
        assert_eq!(
            classify("Pasta.2.4.heic", "Pasta"),
            Some((Slot::Sectioned(2, 4), 4))
        );
        assert_eq!(classify("Dr.Cook.1.jpg", "Dr.Cook"), Some((Slot::Linear(1), 0)));
    }

    #[test]
    fn rejects_other_files() {
        assert_eq!(classify("Pasta.cook", "Pasta"), None);
        assert_eq!(classify("Pasta.gif", "Pasta"), None);
        assert_eq!(classify("Pasta.0.jpg", "Pasta"), None);
        assert_eq!(classify("Pasta.1.2.3.jpg", "Pasta"), None);
        assert_eq!(classify("Pasta.a.jpg", "Pasta"), None);
        assert_eq!(classify("Pasta Salad.jpg", "Pasta"), None);
        assert_eq!(classify("Pasta.Salad.jpg", "Pasta"), None);
        assert_eq!(classify("pasta.jpg", "Pasta"), None);
    }

    #[test]
    fn lists_in_slot_then_extension_order() {
        let (_t, dir) = root();
        let recipe = touch(&dir, "Pasta.cook");
        touch(&dir, "Pasta.2.1.webp");
        touch(&dir, "Pasta.2.jpg");
        touch(&dir, "Pasta.PNG");
        touch(&dir, "Pasta.jpg");
        touch(&dir, "Pasta.1.JPEG");
        touch(&dir, "Pasta.1.2.png");
        touch(&dir, "Pasta Salad.jpg");
        touch(&dir, ".Pasta.jpg");
        touch(&dir, "Pasta.gif");

        assert_eq!(
            names(&companion_files(&recipe)),
            vec![
                "Pasta.jpg",
                "Pasta.PNG",
                "Pasta.1.JPEG",
                "Pasta.2.jpg",
                "Pasta.1.2.png",
                "Pasta.2.1.webp",
            ]
        );
    }

    #[test]
    fn dotted_recipe_name() {
        let (_t, dir) = root();
        let recipe = touch(&dir, "Dr.Cook.cook");
        touch(&dir, "Dr.Cook.jpg");
        touch(&dir, "Dr.Cook.1.jpg");
        touch(&dir, "Dr.jpg");
        assert_eq!(
            names(&companion_files(&recipe)),
            vec!["Dr.Cook.jpg", "Dr.Cook.1.jpg"]
        );
    }

    #[test]
    fn empty_when_nothing_matches_or_folder_is_missing() {
        let (_t, dir) = root();
        let recipe = touch(&dir, "Pasta.cook");
        assert!(companion_files(&recipe).is_empty());
        assert!(companion_files(&dir.join("gone/Pasta.cook")).is_empty());
    }

    #[test]
    fn folders_are_not_companions() {
        let (_t, dir) = root();
        let recipe = touch(&dir, "Pasta.cook");
        fs::create_dir(dir.join("Pasta.jpg")).unwrap();
        assert!(companion_files(&recipe).is_empty());
    }
}
```

- [ ] **Step 2: Wire the module**

In `src/model/mod.rs`:

```rust
pub(crate) mod companions;
pub(crate) mod lossy;
mod metadata;
mod recipe_entry;

pub use companions::{companion_files, IMAGE_EXTENSIONS};
pub use metadata::Metadata;
pub use recipe_entry::{RecipeEntry, RecipeEntryError, StepImageCollection};
```

- [ ] **Step 3: Run the tests**

Run: `cargo test --lib model::companions::`
Expected: 6 passed (the `.Pasta.jpg` hidden file and `Pasta.gif` are skipped; `Pasta.PNG` sorts after `Pasta.jpg` because png ranks after jpg).

- [ ] **Step 4: Lint and commit**

Run: `cargo fmt && make lint`

```bash
git add src/model/companions.rs src/model/mod.rs
git commit -m "feat: companion_files lists a recipe's images in one folder scan"
```

---

## Task 5: `StepImageCollection` with separate linear and sectioned maps; entry uses the scan

**Files:**
- Modify: `src/model/recipe_entry.rs` (imports 1-14; `StepImageCollection` 16-80; struct fields 116-130 and `new` 165-175; `title_image` 270-295; `step_images` 390-440; delete `find_title_image` 489-500, `find_step_images` 517-585, `parse_image_numbers` 587-609; tests)

- [ ] **Step 1: Replace `StepImageCollection`**

Replace lines 16-76 (the doc comment, struct and impl; line 78 starts `RecipeSource`'s doc) with:

```rust
/// A recipe's step and section images, by one-based step number.
///
/// - `RecipeName.N.ext` is step N of a recipe without sections: `linear[N]`
/// - `RecipeName.S.N.ext` is section S, step N: `sectioned[S][N]`
///
/// One path per slot; when several extensions exist for a step the first in
/// [`IMAGE_EXTENSIONS`](crate::IMAGE_EXTENSIONS) order wins.
#[derive(Debug, Clone, Serialize, Default, PartialEq)]
pub struct StepImageCollection {
    /// `Name.N.ext`, keyed by one-based step.
    pub linear: BTreeMap<usize, String>,
    /// `Name.S.N.ext`, keyed by one-based section, then one-based step.
    pub sectioned: BTreeMap<usize, BTreeMap<usize, String>>,
}

impl StepImageCollection {
    /// Returns true if there are no images at all.
    pub fn is_empty(&self) -> bool {
        self.linear.is_empty() && self.sectioned.is_empty()
    }

    /// Total number of images across both kinds.
    pub fn count(&self) -> usize {
        self.linear.len() + self.sectioned.values().map(BTreeMap::len).sum::<usize>()
    }

    /// The image for a step: `section == 0` reads the linear images,
    /// `section >= 1` the sectioned ones. `step == 0` is always `None`.
    ///
    /// # Examples
    /// ```
    /// # use cooklang_find::StepImageCollection;
    /// # let images = StepImageCollection::default();
    /// // Recipe.3.jpg
    /// let img = images.get(0, 3);
    /// // Recipe.2.4.jpg
    /// let img = images.get(2, 4);
    /// ```
    pub fn get(&self, section: usize, step: usize) -> Option<&String> {
        if step == 0 {
            return None;
        }
        if section == 0 {
            self.linear.get(&step)
        } else {
            self.sectioned.get(&section)?.get(&step)
        }
    }

    /// Builds the collection from a sorted companion scan; the first file
    /// per slot wins.
    pub(crate) fn from_companions(companions: &[(Slot, Utf8PathBuf)]) -> Self {
        let mut collection = StepImageCollection::default();
        for (slot, path) in companions {
            match *slot {
                Slot::Title => {}
                Slot::Linear(step) => {
                    collection
                        .linear
                        .entry(step)
                        .or_insert_with(|| path.to_string());
                }
                Slot::Sectioned(section, step) => {
                    collection
                        .sectioned
                        .entry(section)
                        .or_default()
                        .entry(step)
                        .or_insert_with(|| path.to_string());
                }
            }
        }
        collection
    }
}
```

- [ ] **Step 2: Fix the imports**

Replace lines 1-14 with:

```rust
use super::companions::{scan, Slot};
use super::lossy::{lines_lossy, read_to_string_lossy};
use super::metadata::{extract_and_parse_metadata, Metadata};
use crate::menu::Menu;
use camino::{Utf8Path, Utf8PathBuf};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, HashSet};
use std::fs::File;
use std::io::BufReader;
use std::sync::OnceLock;
use thiserror::Error;
```

(`glob`, `regex`, `HashMap` and `std::path::Path` go; `HashSet` stays for `related_files`.)

- [ ] **Step 3: Cache the scan on the entry**

`RecipeEntry` sits inside `tree::DirEntry::Recipe`, and clippy's
`large_enum_variant` fires if the struct grows by more than a few bytes
(the reviewer measured 272 bytes against `Folder`'s 56 with two separate
caches). So the scan and the step images share one boxed cache, which
makes the struct smaller than today, not larger.

Above `impl RecipeEntry`, add:

```rust
/// Image lookups, all from one listing of the recipe's folder.
#[derive(Debug)]
struct Images {
    /// Every companion with its slot, sorted; see `companions::scan`.
    companions: Vec<(Slot, Utf8PathBuf)>,
    /// One path per step slot, built from `companions`.
    steps: StepImageCollection,
}
```

In the `RecipeEntry` struct, replace

```rust
    /// Cached step and section images
    step_images: OnceLock<StepImageCollection>,
```

with

```rust
    /// Cached title and step images, from one folder listing
    images: OnceLock<Box<Images>>,
```

and in `RecipeEntry::new` replace `step_images: OnceLock::new(),` with `images: OnceLock::new(),`.

Add two private accessors right after `fn new`:

```rust
    /// The image cache, filled on first use; empty for content-based entries.
    fn images(&self) -> &Images {
        self.images.get_or_init(|| {
            let companions = match &self.source {
                RecipeSource::Path { path } => scan(path),
                RecipeSource::Content { .. } => Vec::new(),
            };
            let steps = StepImageCollection::from_companions(&companions);
            Box::new(Images { companions, steps })
        })
    }

    /// The companion scan, done once.
    fn companions(&self) -> &[(Slot, Utf8PathBuf)] {
        &self.images().companions
    }
```

- [ ] **Step 4: Point `title_image` and `step_images` at the scan**

In `title_image`, replace the `match &self.source { ... }` with:

```rust
            self.companions()
                .iter()
                .find(|(slot, _)| *slot == Slot::Title)
                .map(|(_, path)| path.to_string())
```

and update its doc line to `/// Supported image extensions: jpg, jpeg, png, webp, heic, in any ASCII case.`

In `step_images`, replace the body with:

```rust
        &self.images().steps
```

and replace the doc example's "Direct HashMap access" block with:

```rust
    /// // Iterate every image
    /// for (step, img) in &images.linear {
    ///     println!("Step {step}: {img}");
    /// }
    /// for (section, steps) in &images.sectioned {
    ///     for (step, img) in steps {
    ///         println!("Section {section}, step {step}: {img}");
    ///     }
    /// }
```

Update the doc comment's extension line to `/// Supported extensions: jpg, jpeg, png, webp, heic (in priority order), any ASCII case`.

- [ ] **Step 5: Add `companion_files` to the entry**

After `step_images`, add:

```rust
    /// The title image and every step image next to this recipe; see
    /// [`companion_files`](crate::companion_files). Empty for content-based
    /// entries. Never a referenced recipe.
    pub fn companion_files(&self) -> Vec<Utf8PathBuf> {
        self.companions()
            .iter()
            .map(|(_, path)| path.clone())
            .collect()
    }
```

- [ ] **Step 6: Delete the old finders**

Delete `find_title_image`, `find_step_images` and `parse_image_numbers` (the three free functions between `read_metadata` and `extract_recipe_references`). Leave `extract_recipe_references` and `collect_related_files` for Task 7, but they no longer compile (they use `find_title_image`/`find_step_images`/`Regex`), so for now change `collect_related_files` to use `scan`:

```rust
    // Collect title and step images
    result.extend(scan(recipe_path).into_iter().map(|(_, path)| path));
```

and in `extract_recipe_references` temporarily keep it compiling by delegating:

```rust
fn extract_recipe_references(content: &str) -> Vec<String> {
    crate::reference::extract_references(content)
}
```

Now that it has a caller, remove the `#[allow(dead_code)]` line above `extract_references` in `src/reference.rs`.

- [ ] **Step 7: Update the tests in `recipe_entry.rs`**

- `test_find_title_image_*` (4 tests around lines 892-960 and 1057): they call `find_title_image(&recipe_path)`. Replace each call with `RecipeEntry::from_path(recipe_path.clone()).unwrap().title_image().clone()` and compare as strings (`title_image()` is `Option<String>`, so the expected side is `Some(image_path.to_string())`); for `test_find_title_image_case_sensitivity`, remove its `#[ignore]` attribute and assert the found path ends with `test_recipe.JPG` (it now passes on Linux too, which is the point).
- `test_step_image_collection_get_zero_step`: build the collection with `collection.linear.insert(1, "test.jpg".to_string());`.
- `test_direct_hashmap_iteration` (line ~1282, the one with `images.images.get(&0)`): replace that block with `assert_eq!(images.linear.keys().copied().collect::<Vec<_>>(), vec![1, 2]);` and rename the test `test_linear_images_iterate_by_step`. `test_recipe_step_images_with_gaps` needs no change.
- `test_recipe_with_section_step_images` (line 1123): keep; it asserts `get(2, 4)` and `get(1, 1)`, which the new storage answers correctly.
- Delete `test_parse_image_numbers_valid` and `test_parse_image_numbers_invalid`; `classify` is tested in `companions.rs`.
- Delete the seven `test_extract_recipe_references_*` tests; `reference.rs` covers them.
- Add:

```rust
    #[test]
    fn test_linear_and_section_one_images_do_not_collide() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "test_recipe", "Test content");
        let linear = create_test_image(&temp_dir_path, "test_recipe.2", "jpg");
        let sectioned = create_test_image(&temp_dir_path, "test_recipe.1.2", "jpg");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        let images = recipe.step_images();

        assert_eq!(images.count(), 2);
        assert_eq!(images.get(0, 2), Some(&linear.to_string()));
        assert_eq!(images.get(1, 2), Some(&sectioned.to_string()));
    }

    #[test]
    fn test_companion_files_on_entry() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let recipe_path = create_test_recipe(&temp_dir_path, "pasta", "Make @./Pesto{}");
        create_test_recipe(&temp_dir_path, "Pesto", "Blend");
        let title = create_test_image(&temp_dir_path, "pasta", "jpg");
        let step = create_test_image(&temp_dir_path, "pasta.1", "heic");

        let recipe = RecipeEntry::from_path(recipe_path).unwrap();
        assert_eq!(recipe.companion_files(), vec![title, step]);

        let content = RecipeEntry::from_content("Make".to_string(), None).unwrap();
        assert!(content.companion_files().is_empty());
    }
```

- [ ] **Step 8: Check the model module compiles on its own**

The lib does not build yet: `src/ffi.rs` still reads `StepImageCollection.images` (fixed in Step 9). Run `cargo build --no-default-features` instead, which leaves `ffi.rs` out.
Expected: builds. Any remaining `.images` reference in `recipe_entry.rs` tests is one missed in Step 7: `grep -n "images\.images\|collection\.images" src/model/recipe_entry.rs` must return nothing.

- [ ] **Step 9: Fix the FFI conversion so the crate compiles**

In `src/ffi.rs`, replace the whole `impl From<&StepImageCollection> for FfiStepImages` (lines 169-206) with:

```rust
impl From<&StepImageCollection> for FfiStepImages {
    fn from(c: &StepImageCollection) -> Self {
        let mut images: Vec<StepImageEntry> = c
            .linear
            .iter()
            .map(|(step, path)| StepImageEntry {
                section: 0,
                step: *step as u32,
                image_path: path.clone(),
            })
            .collect();
        for (section, steps) in &c.sectioned {
            for (step, path) in steps {
                images.push(StepImageEntry {
                    section: *section as u32,
                    step: *step as u32,
                    image_path: path.clone(),
                });
            }
        }
        let count = images.len() as u32;
        FfiStepImages { images, count }
    }
}
```

Update the `StepImageEntry.section` doc to `/// Section number: 0 for a recipe without sections, the one-based section otherwise`.

Replace `test_step_images_conversion` with:

```rust
    #[test]
    fn test_step_images_conversion_keeps_section_one() {
        let mut collection = StepImageCollection::default();
        collection.linear.insert(1, "/r/Pasta.1.jpg".to_string());
        collection.linear.insert(3, "/r/Pasta.3.jpg".to_string());
        collection
            .sectioned
            .entry(1)
            .or_default()
            .insert(2, "/r/Pasta.1.2.jpg".to_string());

        let ffi_images = FfiStepImages::from(&collection);
        assert_eq!(ffi_images.count, 3);
        let listed: Vec<(u32, u32)> = ffi_images
            .images
            .iter()
            .map(|i| (i.section, i.step))
            .collect();
        assert_eq!(listed, vec![(0, 1), (0, 3), (1, 2)]);
    }
```

- [ ] **Step 10: Run everything**

Run: `cargo test`
Expected: all pass, including `model::` (which Step 8 could not run) and the un-ignored case-sensitivity test. (`related_files` tests still pass at this point because `collect_related_files` still resolves from the recipe folder.)

- [ ] **Step 11: Lint and commit**

Run: `cargo fmt && make lint`
Expected: clean. If clippy flags the now-unused `Path` import or similar, remove it.

```bash
git add src/model/recipe_entry.rs src/ffi.rs
git commit -m "fix: keep linear and sectioned step images apart; images from one folder scan"
```

---

## Task 6: `renamed_companions`

**Files:**
- Modify: `src/model/companions.rs`
- Modify: `src/model/mod.rs`

- [ ] **Step 1: Add failing tests**

Append inside `mod tests` of `companions.rs`:

```rust
    fn pairs(renamed: &[RenamedFile]) -> Vec<(String, String)> {
        renamed
            .iter()
            .map(|r| (r.from.to_string(), r.to.to_string()))
            .collect()
    }

    #[test]
    fn rename_in_place() {
        let (_t, dir) = root();
        let recipe = touch(&dir, "Pasta.cook");
        touch(&dir, "Pasta.jpg");
        touch(&dir, "Pasta.2.1.PNG");
        let renamed = renamed_companions(&recipe, &dir.join("Noodles.cook"));
        assert_eq!(
            pairs(&renamed),
            vec![
                (
                    dir.join("Pasta.jpg").to_string(),
                    dir.join("Noodles.jpg").to_string()
                ),
                (
                    dir.join("Pasta.2.1.PNG").to_string(),
                    dir.join("Noodles.2.1.PNG").to_string()
                ),
            ]
        );
    }

    #[test]
    fn move_to_another_folder_and_change_kind() {
        let (_t, dir) = root();
        let recipe = touch(&dir, "Pasta.cook");
        touch(&dir, "Pasta.1.jpg");
        let target = dir.join("Mains/Pasta Bake.menu");
        let renamed = renamed_companions(&recipe, &target);
        assert_eq!(
            pairs(&renamed),
            vec![(
                dir.join("Pasta.1.jpg").to_string(),
                dir.join("Mains/Pasta Bake.1.jpg").to_string()
            )]
        );
    }

    #[test]
    fn nothing_to_rename() {
        let (_t, dir) = root();
        let recipe = touch(&dir, "Pasta.cook");
        touch(&dir, "Pasta.jpg");
        assert!(renamed_companions(&recipe, &recipe).is_empty());
        let bare = touch(&dir, "Bare.cook");
        assert!(renamed_companions(&bare, &dir.join("Other.cook")).is_empty());
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test --lib model::companions::`
Expected: compile error, `RenamedFile`/`renamed_companions` not found.

- [ ] **Step 3: Implement**

Add `use serde::Serialize;` to the imports, then add above `mod tests`:

```rust
/// One companion's current path and where it goes when its recipe is
/// renamed or moved.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RenamedFile {
    pub from: Utf8PathBuf,
    pub to: Utf8PathBuf,
}

/// The companions of the recipe at `from` and their paths once the recipe
/// is at `to` (another name, folder or extension): the old stem is
/// replaced by the new one, in `to`'s folder. Pure path arithmetic after
/// one folder scan; nothing at the destination is checked, and the recipe
/// file itself is not listed. Empty when `from == to`.
pub fn renamed_companions(from: &Utf8Path, to: &Utf8Path) -> Vec<RenamedFile> {
    if from == to {
        return Vec::new();
    }
    let (Some(old_stem), Some(new_stem), Some(new_dir)) =
        (from.file_stem(), to.file_stem(), to.parent())
    else {
        return Vec::new();
    };
    companion_files(from)
        .into_iter()
        .filter_map(|path| {
            let rest = path.file_name()?.strip_prefix(old_stem)?;
            Some(RenamedFile {
                to: new_dir.join(format!("{new_stem}{rest}")),
                from: path,
            })
        })
        .collect()
}
```

- [ ] **Step 4: Export**

In `src/model/mod.rs`: `pub use companions::{companion_files, renamed_companions, RenamedFile, IMAGE_EXTENSIONS};`

- [ ] **Step 5: Run the tests**

Run: `cargo test --lib model::companions::`
Expected: 9 passed.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt && make lint`

```bash
git add src/model/companions.rs src/model/mod.rs
git commit -m "feat: renamed_companions plans the companion moves for a recipe rename"
```

---

## Task 7: `related_files(base_dir)` through `resolve_reference`

**Files:**
- Modify: `src/model/recipe_entry.rs` (`related_files`, `collect_related_files`, delete `extract_recipe_references`; tests ~1368-1525)

- [ ] **Step 1: Update the existing tests to the new signature and rule**

Every `recipe.related_files()` in the tests becomes `recipe.related_files(&temp_dir_path)`, except `test_related_files_content_based_returns_empty`, which has no temp dir: use `recipe.related_files(Utf8Path::new("/nowhere"))` there.

In `test_related_files_recursive`, the Gravy recipe must reference Stock from the root:

```rust
        create_test_recipe(
            &sauces_dir,
            "Gravy",
            "Add @./base/Stock{200%ml} and thicken.",
        );
```

Add after `test_related_files_missing_reference`:

```rust
    #[test]
    fn test_related_files_resolve_from_base_dir_not_recipe_folder() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let sub = temp_dir_path.join("sub");
        std::fs::create_dir_all(&sub).unwrap();

        let root_b = create_test_recipe(&temp_dir_path, "B", "Root B");
        create_test_recipe(&sub, "B", "Sub B");
        let a = create_test_recipe(&sub, "A", "Use @./B{} here.");

        let recipe = RecipeEntry::from_path(a).unwrap();
        assert_eq!(recipe.related_files(&temp_dir_path), vec![root_b]);
    }

    #[test]
    fn test_related_files_follows_menu_references_and_spaces() {
        let temp_dir = TempDir::new().unwrap();
        let temp_dir_path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        let menu = temp_dir_path.join("Sunday.menu");
        std::fs::write(&menu, "= Day\n- @./Pesto Sauce{}\n").unwrap();
        let pesto = create_test_recipe(&temp_dir_path, "Pesto Sauce", "Blend");
        let plan = create_test_recipe(&temp_dir_path, "Plan", "See @./Sunday{} for @./Pesto Sauce{2}.");

        let recipe = RecipeEntry::from_path(plan).unwrap();
        assert_eq!(recipe.related_files(&temp_dir_path), vec![menu, pesto]);
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test --lib model::recipe_entry::tests::test_related`
Expected: compile error, `related_files` takes 0 arguments.

- [ ] **Step 3: Implement**

Replace `related_files` and its doc with:

```rust
    /// Every file this recipe depends on: its companion images, the recipes
    /// and menus it references, and, recursively, theirs.
    ///
    /// References resolve against `base_dir`, the collection root, with
    /// [`resolve_reference`](crate::resolve_reference). Missing references
    /// are skipped; cycles are broken. Empty for content-based entries.
    pub fn related_files(&self, base_dir: &Utf8Path) -> Vec<Utf8PathBuf> {
        let RecipeSource::Path { path } = &self.source else {
            return Vec::new();
        };
        let mut visited = HashSet::new();
        let mut result = Vec::new();
        collect_related_files(base_dir, path, &mut visited, &mut result);
        result
    }
```

Delete `extract_recipe_references` and replace `collect_related_files` with:

```rust
/// Canonical form for the visited set, or the path itself when it cannot
/// be canonicalised.
fn canonical(path: &Utf8Path) -> Utf8PathBuf {
    match std::fs::canonicalize(path) {
        Ok(p) => Utf8PathBuf::from_path_buf(p)
            .unwrap_or_else(|p| Utf8PathBuf::from(p.to_string_lossy().into_owned())),
        Err(_) => path.to_path_buf(),
    }
}

/// Recursively collects the files related to `recipe_path`: companions,
/// then each resolved reference and its own related files.
fn collect_related_files(
    base_dir: &Utf8Path,
    recipe_path: &Utf8Path,
    visited: &mut HashSet<Utf8PathBuf>,
    result: &mut Vec<Utf8PathBuf>,
) {
    if !visited.insert(canonical(recipe_path)) {
        return;
    }
    result.extend(scan(recipe_path).into_iter().map(|(_, path)| path));

    // Decoded lossily, so a stray non-UTF-8 byte does not hide references.
    let Ok(content) =
        File::open(recipe_path).and_then(|file| read_to_string_lossy(&mut BufReader::new(file)))
    else {
        return;
    };
    for reference in extract_references(&content) {
        let Some(file) = resolve_reference(base_dir, &reference) else {
            continue;
        };
        if !visited.contains(&canonical(&file)) {
            result.push(file.clone());
            collect_related_files(base_dir, &file, visited, result);
        }
    }
}
```

Add to the imports: `use crate::reference::{extract_references, resolve_reference};`

- [ ] **Step 4: Run the model tests**

Run: `cargo test --lib model::`
Expected: all pass.

- [ ] **Step 5: Fix the FFI call site so the crate compiles**

In `src/ffi.rs`, replace `FfiRecipeEntry::related_files`:

```rust
    /// Returns every file this recipe depends on: its companion images, the
    /// recipes and menus it references (resolved against `base_dir`, the
    /// collection root), and recursively theirs.
    pub fn related_files(&self, base_dir: String) -> Vec<String> {
        self.inner
            .related_files(Utf8Path::new(&base_dir))
            .into_iter()
            .map(|p| p.to_string())
            .collect()
    }
```

In `test_related_files_ffi`, change the call to `recipe.related_files(temp_path.to_string())`.

- [ ] **Step 6: Run everything, lint, commit**

Run: `cargo test`, then `cargo fmt && make lint`
Expected: all pass, lint clean.

```bash
git add src/model/recipe_entry.rs src/ffi.rs
git commit -m "feat!: related_files resolves references from the collection root"
```

---

## Task 8: Menu scaling resolves through `resolve_reference`

**Files:**
- Modify: `src/menu/scale.rs:1-10, 76-98`

- [ ] **Step 1: Add a failing test**

Append inside `mod tests` of `scale.rs`:

```rust
    #[test]
    fn explicit_cook_suffix_in_any_case_is_looked_up_once() {
        let (_t, dir) = recipes_dir();
        assert_eq!(
            resolve("@./Pancakes.COOK{4%servings}", &dir, 1.0),
            vec![Some(2.0)]
        );
    }

    #[test]
    fn reference_escaping_the_root_is_missing() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@../Pancakes{4%servings}", &dir, 1.0), vec![Some(4.0)]);
    }
```

- [ ] **Step 2: Run to see the first fail**

Run: `cargo test --lib menu::scale::`
Expected: `explicit_cook_suffix_in_any_case_is_looked_up_once` fails (today `Pancakes.COOK` becomes `Pancakes.COOK.cook`, which is missing, so the raw `4` comes back).

- [ ] **Step 3: Implement**

Replace `RecipeSize::load` and its doc with:

```rust
    /// Resolves `path` with [`resolve_reference`] in each base dir in turn
    /// and reads the first match's metadata; a missing recipe is a default
    /// (raw-multiplier) size.
    fn load<P: AsRef<Utf8Path>>(base_dirs: &[P], path: &str) -> Self {
        base_dirs
            .iter()
            .find_map(|dir| resolve_reference(dir.as_ref(), path))
            .and_then(|file| RecipeEntry::from_path(file).ok())
            .map(|entry| Self::from_metadata(entry.metadata()))
            .unwrap_or_default()
    }
```

Change the imports: remove `use crate::fetcher::get_recipe;`, add `use crate::model::RecipeEntry;` and `use crate::reference::resolve_reference;`.

Update the `resolve_scales` doc paragraph that starts "Reference paths are resolved against `base_dirs`" to:

```rust
    /// Reference paths are resolved against `base_dirs` (the library root),
    /// not the menu's own folder, with [`resolve_reference`]: `@./Mains/Stew`
    /// is looked up as `<base_dir>/Mains/Stew.cook`, then `.menu`, in each
    /// base dir in turn; `@./Weekly.menu` as `<base_dir>/Weekly.menu`. A
    /// recipe is only loaded when the result depends on it (a numeric target
    /// with a unit), at most once per call.
```

- [ ] **Step 4: Run the menu tests**

Run: `cargo test --lib menu::`
Expected: all pass, including `dotted_reference_name_is_not_an_extension`, `explicit_menu_reference_is_looked_up_as_is`, `cook_file_wins_over_menu_file`.

- [ ] **Step 5: Lint and commit**

Run: `cargo fmt && make lint`

```bash
git add src/menu/scale.rs
git commit -m "fix: menu scaling resolves references with the shared rule"
```

---

## Task 9: FFI exports

**Files:**
- Modify: `src/ffi.rs` (imports 6-22; records after `FfiDirListing` ~413; free functions before `library_version` ~850; tests)

- [ ] **Step 1: Add failing FFI tests**

Append inside `mod tests`:

```rust
    #[test]
    fn test_companion_files_ffi() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();
        let path = create_test_recipe(temp_path, "Pasta", "Boil");
        fs::write(format!("{temp_path}/Pasta.JPG"), b"").unwrap();
        fs::write(format!("{temp_path}/Pasta.1.2.png"), b"").unwrap();

        let expected = vec![
            format!("{temp_path}/Pasta.JPG"),
            format!("{temp_path}/Pasta.1.2.png"),
        ];
        assert_eq!(companion_files(path.clone()), expected);
        assert_eq!(recipe_from_path(path).unwrap().companion_files(), expected);
    }

    #[test]
    fn test_renamed_companions_ffi() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();
        let path = create_test_recipe(temp_path, "Pasta", "Boil");
        fs::write(format!("{temp_path}/Pasta.1.jpg"), b"").unwrap();

        let renamed = renamed_companions(path, format!("{temp_path}/Noodles.cook"));
        assert_eq!(renamed.len(), 1);
        assert_eq!(renamed[0].from, format!("{temp_path}/Pasta.1.jpg"));
        assert_eq!(renamed[0].to, format!("{temp_path}/Noodles.1.jpg"));
    }

    #[test]
    fn test_resolve_reference_ffi() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();
        let pesto = create_test_recipe(temp_path, "Pesto", "Blend");

        assert_eq!(
            resolve_reference(temp_path.to_string(), "./Pesto".to_string()),
            Some(pesto)
        );
        assert_eq!(
            resolve_reference(temp_path.to_string(), "../Pesto".to_string()),
            None
        );
    }

    #[test]
    fn test_get_step_image_section_numbers() {
        let temp_dir = TempDir::new().unwrap();
        let temp_path = temp_dir.path().to_str().unwrap();
        let path = create_test_recipe(temp_path, "Pasta", "Boil");
        fs::write(format!("{temp_path}/Pasta.2.jpg"), b"").unwrap();
        fs::write(format!("{temp_path}/Pasta.1.2.jpg"), b"").unwrap();

        let recipe = recipe_from_path(path).unwrap();
        assert_eq!(
            recipe.get_step_image(0, 2),
            Some(format!("{temp_path}/Pasta.2.jpg"))
        );
        assert_eq!(
            recipe.get_step_image(1, 2),
            Some(format!("{temp_path}/Pasta.1.2.jpg"))
        );
        let sections: Vec<u32> = recipe.step_images().images.iter().map(|i| i.section).collect();
        assert_eq!(sections, vec![0, 1]);
    }
```

- [ ] **Step 2: Run to see them fail**

Run: `cargo test --lib ffi::`
Expected: compile errors for the three missing free functions and `companion_files` on the entry.

- [ ] **Step 3: Implement**

Imports: change the `crate::model` line to

```rust
use crate::model::{
    companion_files as companion_files_internal, renamed_companions as renamed_companions_internal,
    Metadata, RecipeEntry, RecipeEntryError, StepImageCollection,
};
use crate::reference::resolve_reference as resolve_reference_internal;
```

In `impl FfiRecipeEntry` (the `#[uniffi::export]` block), after `step_images`:

```rust
    /// The title image and every step image next to this recipe, title
    /// first, then by section and step. Never a referenced recipe. Empty
    /// for content-based entries.
    pub fn companion_files(&self) -> Vec<String> {
        self.inner
            .companion_files()
            .into_iter()
            .map(|p| p.to_string())
            .collect()
    }
```

After `FfiDirListing`, add the record:

```rust
/// A companion file's current path and where it goes when its recipe is
/// renamed or moved.
#[derive(Debug, Clone, uniffi::Record)]
pub struct FfiRenamedFile {
    pub from: String,
    pub to: String,
}
```

Before `library_version`, add:

```rust
/// Lists the images that belong to the recipe at `recipe_path`: `Name.ext`
/// (title), `Name.N.ext` (step N, no sections) and `Name.S.N.ext` (section
/// S, step N), one-based, extensions jpg/jpeg/png/webp/heic in any case.
/// Title first, then by section and step; every extension variant of a slot
/// is included. Opens no file. Empty when the folder cannot be read.
#[uniffi::export]
pub fn companion_files(recipe_path: String) -> Vec<String> {
    companion_files_internal(Utf8Path::new(&recipe_path))
        .into_iter()
        .map(|p| p.to_string())
        .collect()
}

/// For each companion of the recipe at `from`, where it goes when the
/// recipe moves to `to`: old stem replaced by the new stem, in `to`'s
/// folder. The caller performs the moves; nothing at the destination is
/// checked. Empty when `from == to`.
#[uniffi::export]
pub fn renamed_companions(from: String, to: String) -> Vec<FfiRenamedFile> {
    renamed_companions_internal(Utf8Path::new(&from), Utf8Path::new(&to))
        .into_iter()
        .map(|r| FfiRenamedFile {
            from: r.from.to_string(),
            to: r.to.to_string(),
        })
        .collect()
}

/// Resolves a `@./path` reference against the collection root: `.` and
/// `..` segments are normalised, a path leaving the root is `nil`, a
/// `.cook`/`.menu` suffix is tried as written, then `<path>.cook` and
/// `<path>.menu`. The first existing file, or `nil`.
#[uniffi::export]
pub fn resolve_reference(base_dir: String, reference: String) -> Option<String> {
    resolve_reference_internal(Utf8Path::new(&base_dir), &reference).map(|p| p.to_string())
}
```

- [ ] **Step 4: Run the FFI tests**

Run: `cargo test --lib ffi::`
Expected: 20 passed (16 existing, 4 new).

- [ ] **Step 5: Check the Swift bindings generate**

Run: `./scripts/build-swift.sh --generate-only > /tmp/bindgen.log 2>&1; tail -5 /tmp/bindgen.log; grep -n "func companionFiles\|func renamedCompanions\|func resolveReference\|relatedFiles(baseDir" bindings/swift/Sources/CooklangFind/CooklangFind.swift | head`
Expected: the symbols appear (`companionFiles()`, `relatedFiles(baseDir:)` on the entry; `companionFiles(recipePath:)`, `renamedCompanions(from:to:)`, `resolveReference(baseDir:reference:)` free). The script writes under `bindings/`, which is untracked and not git-ignored: do not `git add` it. The committed `Sources/CooklangFind` is regenerated by the release workflow, so do not touch it.

- [ ] **Step 6: Lint and commit**

Run: `cargo fmt && make lint`

```bash
git add src/ffi.rs
git commit -m "feat(ffi): companion_files, renamed_companions, resolve_reference"
```

---

## Task 10: Docs

**Files:**
- Modify: `README.md` (Features list ~5-13; new sections after "Working with Menus" ~158-248; "Title Image Support" 294-298)
- Modify: `BINDINGS.md` (Functions table 266-282; `FfiRecipeEntry` table 286-300; `StepImageEntry` 319-325; new `FfiRenamedFile`)
- Modify: `src/lib.rs` crate doc (lines 5-12)

- [ ] **Step 1: README**

In the Features bullet list add:

```markdown
- Companion files: the title and step images that belong to a recipe, and where they go on a rename
- Recipe references (`@./path`) resolved with one root-relative rule
```

After the "Working with Menus" section add:

```markdown
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
let recipe = RecipeEntry::from_path(Utf8PathBuf::from("recipes/Pasta.cook"))?;
let files = recipe.related_files(root);
# Ok::<(), Box<dyn std::error::Error>>(())
```
```

Replace the "Title Image Support" section with:

```markdown
### Images
- Title image: `Name.ext` next to the recipe
- Step images: `Name.N.ext` (step N, one-based, no sections) and
  `Name.S.N.ext` (section S, step N, both one-based); `StepImageCollection`
  keeps the two apart, and section 0 means "no sections"
- Extensions jpg, jpeg, png, webp, heic, matched in any letter case; when a
  slot has several, that order decides
- One folder listing per recipe, shared by `title_image`, `step_images`
  and `companion_files`
```

- [ ] **Step 2: BINDINGS.md**

Add rows to the Functions table:

```markdown
| `companionFiles(recipePath)` | The images that belong to a recipe (`Name.ext`, `Name.N.ext`, `Name.S.N.ext`; jpg/jpeg/png/webp/heic in any case), title first then by section and step. Opens no file |
| `renamedCompanions(from, to)` | For each companion of `from`, its path once the recipe is at `to` (old stem swapped for the new, in `to`'s folder). The caller moves the files; empty when `from == to` |
| `resolveReference(baseDir, reference)` | The file a `@./path` reference means, resolved against the collection root: `.cook` then `.menu`, a `.cook`/`.menu` suffix as written first, `nil` when missing or outside the root |
```

Update the `parseMenu` row's resolution text to "References resolve with `resolveReference` against each of `baseDirs` in turn".

In the `FfiRecipeEntry` table add:

```markdown
| `companionFiles()` | `List<String>` | Title and step images next to this recipe |
| `relatedFiles(baseDir)` | `List<String>` | Companions plus referenced recipes and menus, recursively; references resolve against `baseDir` |
```

Change the `StepImageEntry.section` row to `Section number (0 = no sections; otherwise the one-based section)`.

After `StepImageEntry` add:

```markdown
#### FfiRenamedFile

| Field | Type | Description |
|-------|------|-------------|
| `from` | `String` | The companion's current path |
| `to` | `String` | Its path after the rename |
```

- [ ] **Step 3: Crate doc**

In `src/lib.rs` add to the bullet list:

```rust
//! - Companion files: the images that belong to a recipe, and where they
//!   go on a rename
//! - Resolving `@./path` references against the collection root
```

- [ ] **Step 4: Doc tests and commit**

Run: `cargo test --doc`
Expected: pass (the README code is not compiled; the `lib.rs` and `StepImageCollection::get` doc examples are).

```bash
git add README.md BINDINGS.md src/lib.rs
git commit -m "docs: companion files, references and the image numbering rule"
```

---

## Task 11: Full verification, pull request, release

- [ ] **Step 1: Full test and lint**

Run: `cargo test > /tmp/cooklang-find-tests.log 2>&1; tail -15 /tmp/cooklang-find-tests.log; cargo fmt && make lint`
Expected: every suite `test result: ok`; lint clean. Also `cargo build --no-default-features` must succeed (Rust-only consumers).

- [ ] **Step 2: Confirm nothing still reads the old shapes**

Run: `grep -rn "fn find_title_image\|fn find_step_images\|fn parse_image_numbers\|fn extract_recipe_references\|HashMap<usize\|use glob::glob\|use regex::Regex" src/model src/ffi.rs`
Expected: no output. (`self.images` in `recipe_entry.rs` and `FfiStepImages.images` in `ffi.rs` are the new names and are fine.)

- [ ] **Step 3: Open the pull request**

```bash
git push -u origin companion-files-and-references
gh pr create --title "Companion files, one reference rule, step image sections" --body "$(cat <<'EOF'
Spec: docs/superpowers/specs/2026-10-14-companion-files-and-references-design.md

- `companion_files`, `renamed_companions`, `resolve_reference` (Rust + FFI)
- `related_files` now takes `base_dir` and resolves from the collection root, like `parse_menu`
- the menu parser's component scanner is shared with recipe content (names with spaces and dots)
- `StepImageCollection` keeps linear and sectioned images apart; the FFI reports section 1 as 1
- image extensions matched in any case; `heic` added

Breaking: `related_files(base_dir)`, `StepImageCollection { linear, sectioned }` (also its JSON), FFI step image `section` for sectioned images.
Breaking also: menu scaling no longer follows a reference outside the root; image stems match exactly; `Name.01.jpg` is not a step image; references in comments/frontmatter are ignored.
EOF
)"
```

Wait for CI (ubuntu, macos, windows × stable, beta) to pass, then squash-merge.

- [ ] **Step 4: Release 0.11.0**

Run the `Release` workflow from `main` with version `0.11.0` (`gh workflow run release.yml -f version=0.11.0`). It bumps `Cargo.toml`, builds the XCFramework and Kotlin bindings, rewrites `Package.swift` with the checksum, tags `v0.11.0`, publishes the GitHub release and the crate. Then edit the release notes to add:

```
### Breaking
- `relatedFiles` now takes `baseDir` and resolves `@./path` references from the collection root (the same rule as `parseMenu`), no longer from the recipe's own folder.
- Step images: `StepImageCollection` now has `linear` and `sectioned` maps (its JSON shape changed), and the FFI reports a sectioned image's real section, so `Name.1.2.jpg` is section 1 (it used to collapse into section 0).
- `parseMenu` no longer loads a referenced recipe from outside the base dirs (`@../x` is treated as missing) and now resolves `@./Name.cook` and `@./Name.COOK`.
- Image file stems must match the recipe's stem exactly (`pasta.jpg` is no longer the title image of `Pasta.cook` on a case-insensitive filesystem), and `Name.01.jpg` (leading zero) is not a step image.
- `relatedFiles` ignores references inside frontmatter and comments, follows `.menu` references, and lists each file once.
- `StepImageCollection` JSON: `images` (zero-based nested maps) is replaced by `linear` and `sectioned` with one-based keys.

### Added
- `companionFiles`, `renamedCompanions`, `resolveReference`; `FfiRecipeEntry.companionFiles()`.
- Image extensions matched in any letter case; `heic` is recognised.
- Recipe references with spaces or dots in the name (`@./Pesto Sauce{}`, `@./Dr.Cook{}`) are followed.
```

Verify with `gh release view v0.11.0` and `cargo search cooklang-find`.

- [ ] **Step 5: Follow-ups outside this repo** (not part of this plan's checklist, recorded so they are not lost)

- Spec repo PR: README "Adding Pictures" lists the extension set and uses one-based `Chicken French.1.jpg` in the example.
- iOS app: pin 0.11.0 and migrate under its own spec.
