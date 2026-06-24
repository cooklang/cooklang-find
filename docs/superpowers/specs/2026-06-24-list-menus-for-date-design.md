# Design: `list_menus_for_date`

**Date:** 2026-06-24
**Status:** Approved

## Overview

Add the ability to find Cooklang menu (`.menu`) files that are active on a given
date. A menu file is considered active for a date when one of its section headers
contains that date string (e.g. a section `= 2026-06-24 Dinner` makes the file
active for `2026-06-24`).

The library performs no date parsing, timezone handling, or arithmetic. The
caller supplies the date as an opaque string and the library matches it
literally. This keeps the crate dependency-free (no `chrono`) and trivially
testable. The host application (e.g. the mobile app) computes its own local
"today" / "tomorrow" and calls this function once per date.

## API

### Rust core

```rust
pub fn list_menus_for_date<P: AsRef<Utf8Path>>(
    base_dirs: &[P],
    date: &str,
) -> Result<Vec<RecipeEntry>, MenuError>
```

- `base_dirs` — one or more root directories to scan. A single directory is just
  a one-element slice. Mirrors the multi-directory style of `get_recipe`.
- `date` — opaque date string (e.g. `"2026-06-24"`), matched literally as a
  substring of section header names. No validation or parsing.
- Returns `RecipeEntry` values, consistent with `search` / `get_recipe`.

Exported from `lib.rs`:

```rust
pub mod menu;
pub use menu::list_menus_for_date;
```

### FFI wrapper

Added to `src/ffi.rs` alongside the existing `search` / `get_recipe` wrappers,
following the same conversion patterns:

```rust
#[uniffi::export]
pub fn list_menus_for_date(
    base_dirs: Vec<String>,
    date: String,
) -> Result<Vec<Arc<FfiRecipeEntry>>, CooklangError>
```

Returns `Vec<Arc<FfiRecipeEntry>>`, the same representation used by the existing
`search` wrapper. `MenuError` is converted into the unified FFI `CooklangError`
via a `From<MenuError>` impl, consistent with how `SearchError` is mapped.

## Module structure

New module `src/menu/mod.rs`, parallel to the existing `search/`, `fetcher/`,
and `tree/` modules.

## Matching logic

The library does not use the Cooklang parser; it scans raw file content with
manual line scanning, consistent with the existing metadata-extraction approach.

For each base directory:

1. Glob `**/*.menu` (reusing the existing glob pattern style). Only `.menu`
   files are scanned; `.cook` recipe files are ignored.
2. Read each file's content and scan it line by line. A **section header** is a
   line whose trimmed form starts with `=`. The section name is obtained by
   stripping leading/trailing `=` characters and surrounding whitespace.
3. If **any** section name **contains** the `date` substring, include the file
   as a `RecipeEntry`.
4. De-duplicate files so the same path is never returned twice (relevant when
   base directories overlap).

A date that appears only in a step or body line (not a section header) does not
cause a match.

## Error handling

New `MenuError` enum, mirroring `SearchError`:

- Glob pattern error
- IO / invalid-UTF-8 path error
- Wrapped `RecipeEntryError`

Individual unreadable files are **skipped** rather than failing the whole call,
consistent with commit `cc439ab` ("skip unreadable files in build_tree instead
of failing").

## Testing

Unit tests using `tempfile` + `indoc`, matching the existing test style:

- Menu with a section header exactly the date → included.
- Section header containing the date plus a label (`= 2026-06-24 Dinner`) →
  included.
- Menu with no matching date → excluded.
- `.cook` file containing the date in its text → excluded (menus only).
- Date appearing in a step/body line but not in a section header → excluded.
- Multiple base directories → results merged.
- No matches → empty vector.

## Out of scope

- Date parsing, validation, or formatting.
- Timezone handling and computing "today" / "tomorrow" (caller's responsibility).
- Named `today` / `tomorrow` convenience methods (caller calls the function
  twice with the two dates).
- Adding a `chrono` dependency.
