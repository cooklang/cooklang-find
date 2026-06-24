//! Menu discovery by date.
//!
//! This module finds Cooklang `.menu` files that are "active" on a given date.
//! A menu file is active for a date when one of its section headers contains
//! that date string (for example, a section `= 2026-06-24 Dinner` makes the
//! file active for `2026-06-24`).
//!
//! The library performs no date parsing or timezone handling: the caller
//! supplies the date as an opaque string that is matched literally.

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
