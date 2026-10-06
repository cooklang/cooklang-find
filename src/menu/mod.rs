//! Menu files: discovery by date and structured parsing.
//!
//! [`list_menus_for_date`] finds `.menu` files that are "active" on a given
//! date. A menu file is active for a date when one of its section headers
//! contains that date string (for example, a section `= 2026-06-24 Dinner`
//! makes the file active for `2026-06-24`).
//!
//! [`list_menus_between`] finds the menus with a section dated in a range,
//! in one walk, and returns each parsed.
//!
//! [`Menu`] is the structured form of a menu: sections, meals, and items.
//!
//! The library performs no date parsing or timezone handling: the caller
//! supplies the date as an opaque string that is matched literally.

mod model;
mod parse;
mod scale;

pub use model::{Menu, MenuItem, MenuMeal, MenuSection};

use crate::model::lossy::read_to_string_lossy;
use crate::model::RecipeEntry;
use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
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
    parse::section_name(line).is_some_and(|name| name.contains(date))
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
    let mut menus = Vec::new();
    for_each_menu_file(base_dirs, |path, content| {
        if content
            .lines()
            .any(|line| section_header_contains_date(line, date))
        {
            // Skip files that can't be parsed.
            if let Ok(recipe) = RecipeEntry::from_path(path) {
                menus.push(recipe);
            }
        }
    })?;
    Ok(menus)
}

/// A menu file found by [`list_menus_between`], with its parsed content.
#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct MenuMatch {
    /// The `.menu` file.
    pub path: Utf8PathBuf,
    /// Its parsed content. Scales are not resolved; call
    /// [`Menu::resolve_scales`] if needed.
    pub menu: Menu,
}

/// Returns the `.menu` files under `base_dirs` with a section dated between
/// `from` and `to` (inclusive), each with its parsed [`Menu`].
///
/// A section's date is its parsed [`MenuSection::date`] (the first
/// `YYYY-MM-DD` in its header), and dates are compared as strings, which
/// orders ISO dates correctly. Pass the same date twice for a single day.
///
/// Each file is read once, however many of its sections match, and parsed
/// from that same read; no referenced recipe is opened. A file is returned
/// once even if base directories overlap. Unreadable files are skipped.
/// The menu's name falls back to the file stem.
///
/// Unlike [`list_menus_for_date`], which matches a literal substring of the
/// header, this only sees ISO dates.
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::list_menus_between;
/// use camino::Utf8Path;
///
/// // Today and tomorrow, for an "upcoming meal plans" view.
/// let matches = list_menus_between(&[Utf8Path::new("./menus")], "2026-06-24", "2026-06-25")?;
/// for m in &matches {
///     println!("{} {:?} {:?}", m.path, m.menu.name, m.menu.date_range());
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn list_menus_between<P: AsRef<Utf8Path>>(
    base_dirs: &[P],
    from: &str,
    to: &str,
) -> Result<Vec<MenuMatch>, MenuError> {
    let mut matches = Vec::new();
    for_each_menu_file(base_dirs, |path, content| {
        let fallback_name = path.file_stem().unwrap_or_default();
        let menu = Menu::parse(&content, fallback_name);
        let in_range = menu.dates().iter().any(|date| (from..=to).contains(date));
        if in_range {
            matches.push(MenuMatch { path, menu });
        }
    })?;
    Ok(matches)
}

/// Calls `f` with the path and content of every `.menu` file under
/// `base_dirs`, once per path even if base directories overlap. Files whose
/// content can't be read are skipped. Content is decoded lossily, so a menu
/// with a stray non-UTF-8 byte is still seen.
fn for_each_menu_file<P: AsRef<Utf8Path>>(
    base_dirs: &[P],
    mut f: impl FnMut(Utf8PathBuf, String),
) -> Result<(), MenuError> {
    let mut seen: HashSet<Utf8PathBuf> = HashSet::new();
    for base_dir in base_dirs {
        let pattern = base_dir.as_ref().join("**/*.menu").to_string();
        for entry in crate::walk::glob_visible(&pattern)? {
            let path = Utf8PathBuf::from_path_buf(entry?).map_err(|_| {
                MenuError::IoError(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    "Path contains invalid UTF-8",
                ))
            })?;
            if !seen.insert(path.clone()) {
                continue;
            }
            let Ok(content) = std::fs::File::open(&path)
                .and_then(|file| read_to_string_lossy(&mut std::io::BufReader::new(file)))
            else {
                continue;
            };
            f(path, content);
        }
    }
    Ok(())
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

    #[test]
    fn skips_hidden_menu_files() {
        // https://github.com/cooklang/cookcli/issues/555
        let tmp = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(tmp.path().to_path_buf()).unwrap();
        fs::write(dir.join("week.menu"), "= 2026-06-24\n@./a{}\n").unwrap();
        fs::write(dir.join("._week.menu"), "= 2026-06-24\n@./a{}\n").unwrap();

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path().unwrap(), &dir.join("week.menu"));
    }

    #[test]
    fn recipe_entry_menu_parses_menu_files() {
        let (_t, dir) = temp_dir();
        write_file(
            &dir,
            "week.menu",
            "= Day 1 (2026-06-24)\nDinner:\n- @./Stew{}\n",
        );
        write_file(&dir, "stew.cook", "@beef{}\n");

        let menu = RecipeEntry::from_path(dir.join("week.menu"))
            .unwrap()
            .menu()
            .unwrap()
            .unwrap();
        let not_menu = RecipeEntry::from_path(dir.join("stew.cook"))
            .unwrap()
            .menu();

        assert_eq!(menu.name, "week");
        assert_eq!(menu.dates(), vec!["2026-06-24"]);
        assert!(not_menu.is_none());
    }

    #[test]
    fn finds_menus_with_non_lowercase_extension() {
        // https://github.com/cooklang/cooklang-find/issues/11
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.MENU", "= 2026-06-24\n\n@a{}\n");

        let results = list_menus_for_date(&[&dir], "2026-06-24").unwrap();

        assert_eq!(results.len(), 1);
        assert!(results[0].is_menu());
    }

    #[test]
    fn between_returns_path_and_parsed_menu() {
        let (_t, dir) = temp_dir();
        write_file(
            &dir,
            "week.menu",
            "= Mon (2026-06-22)\nDinner:\n- @./Soup{2}\n= Tue (2026-06-23)\n",
        );

        let matches = list_menus_between(&[&dir], "2026-06-23", "2026-06-24").unwrap();

        assert_eq!(matches.len(), 1);
        assert_eq!(matches[0].path, dir.join("week.menu"));
        let menu = &matches[0].menu;
        assert_eq!(menu.name, "week");
        assert_eq!(menu.date_range(), Some(("2026-06-22", "2026-06-23")));
        assert!(matches!(
            menu.recipe_references()[0],
            MenuItem::RecipeReference { scale: None, .. }
        ));
    }

    #[test]
    fn between_is_inclusive_and_excludes_out_of_range() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "before.menu", "= 2026-06-21\n");
        write_file(&dir, "first.menu", "= 2026-06-22\n");
        write_file(&dir, "last.menu", "= Day (2026-06-24)\n");
        write_file(&dir, "after.menu", "= 2026-06-25\n");

        let matches = list_menus_between(&[&dir], "2026-06-22", "2026-06-24").unwrap();

        let mut names: Vec<_> = matches.iter().map(|m| m.menu.name.as_str()).collect();
        names.sort();
        assert_eq!(names, vec!["first", "last"]);
    }

    #[test]
    fn between_returns_a_menu_once_for_several_matching_dates() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= 2026-06-24\n= 2026-06-25\n");

        let matches = list_menus_between(&[&dir, &dir], "2026-06-24", "2026-06-25").unwrap();

        assert_eq!(matches.len(), 1);
    }

    #[test]
    fn between_uses_parsed_section_dates_only() {
        let (_t, dir) = temp_dir();
        write_file(
            &dir,
            "body.menu",
            "= Dinner\nMade on 2026-06-24 with @eggs{}\n",
        );
        write_file(&dir, "recipe.cook", "= 2026-06-24\n");

        let matches = list_menus_between(&[&dir], "2026-06-24", "2026-06-24").unwrap();

        assert!(matches.is_empty());
    }

    #[test]
    fn between_merges_base_dirs_and_uses_title() {
        let (_t1, dir1) = temp_dir();
        let (_t2, dir2) = temp_dir();
        write_file(&dir1, "a.menu", "---\ntitle: Summer\n---\n= 2026-06-24\n");
        write_file(&dir2, "b.menu", "= 2026-06-25\n");

        let matches = list_menus_between(&[&dir1, &dir2], "2026-06-24", "2026-06-25").unwrap();

        let names: Vec<_> = matches.iter().map(|m| m.menu.name.as_str()).collect();
        assert_eq!(names, vec!["Summer", "b"]);
    }

    #[test]
    fn between_with_reversed_range_is_empty() {
        let (_t, dir) = temp_dir();
        write_file(&dir, "week.menu", "= 2026-06-24\n");

        let matches = list_menus_between(&[&dir], "2026-06-25", "2026-06-24").unwrap();

        assert!(matches.is_empty());
    }
}
