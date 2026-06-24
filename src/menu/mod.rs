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

/// Returns all `.menu` files under `base_dirs` that have a section header
/// containing `date`.
///
/// Only `.menu` files are scanned; `.cook` recipe files are ignored. A file is
/// included if **any** of its section headers contains the `date` substring.
/// Dates appearing only in step/body lines do not cause a match. Files that
/// cannot be read are silently skipped, as are matching files that fail to
/// parse. The same path is never returned twice, even if base directories
/// overlap.
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

            let content = match std::fs::read_to_string(&path) {
                Ok(c) => c,
                Err(_) => continue, // Skip files whose content isn't available
            };

            if content
                .lines()
                .any(|line| section_header_contains_date(line, date))
            {
                match RecipeEntry::from_path(path) {
                    Ok(recipe) => menus.push(recipe),
                    Err(_) => continue, // Skip files that can't be parsed
                }
            }
        }
    }

    Ok(menus)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use tempfile::TempDir;

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
        write_file(
            &dir,
            "week.menu",
            "= Dinner\n\nMade on 2026-06-24 with @eggs{}\n",
        );

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

    #[test]
    fn dedups_overlapping_base_dirs() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= 2026-06-24\n\n@a{}\n");

        // Same directory passed twice -> the file must appear only once.
        let results = list_menus_for_date(&[&dir, &dir], "2026-06-24").unwrap();

        assert_eq!(results.len(), 1);
    }

    #[test]
    fn finds_menus_in_subdirectories() {
        let (_t, dir) = temp_dir();
        let nested = dir.join("plans/june");
        fs::create_dir_all(&nested).unwrap();
        write_file(&nested, "week.menu", "= 2026-06-24\n\n@a{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert_eq!(results.len(), 1);
    }
}
