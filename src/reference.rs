//! Recipe references (`@./path`), shared by `.cook` and `.menu` handling:
//! the component scanner, reference extraction and reference resolution.
//!
//! The scanner is the menu parser's rule, used for recipe content too so a
//! `@./path` on a line means the same file in either kind of file. The
//! single-word fallback is deliberately more lenient than cooklang-rs, whose
//! lexer breaks a word at `.` and `/` and so never accepts a braceless
//! `@./Pesto`; do not "fix" it to match the parser.

use std::collections::HashSet;

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
        assert_eq!(name("./Pesto {2}"), Some("./Pesto"));
    }

    #[test]
    fn bare_name_is_one_word_and_keeps_inner_dots() {
        assert_eq!(name("./Dr.Cook and more"), Some("./Dr.Cook"));
        assert_eq!(name("./Sunday.menu"), Some("./Sunday.menu"));
        assert_eq!(name("./Pesto Sauce"), Some("./Pesto"));
        assert_eq!(name("./Pesto, then"), Some("./Pesto"));
    }

    #[test]
    fn trailing_dot_is_punctuation() {
        assert_eq!(name("./Pesto."), Some("./Pesto"));
    }

    #[test]
    fn another_component_before_the_brace_forces_single_word() {
        assert_eq!(name("./Pesto Sauce and @salt{}"), Some("./Pesto"));
        assert_eq!(name("./Pesto #pan{} x{}"), Some("./Pesto"));
        assert_eq!(name("./Pesto -- x{}"), Some("./Pesto"));
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
    fn nothing_word_like_is_not_a_component() {
        assert_eq!(component("-- note"), None);
        assert_eq!(component(" noon"), None);
    }

    #[test]
    fn is_reference_needs_a_dot_slash_prefix() {
        assert!(is_reference("./x"));
        assert!(is_reference("../x"));
        assert!(!is_reference("x"));
        assert!(!is_reference(".x"));
    }

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
        assert_eq!(extract_references("@./Pesto\nthen {stir}"), vec!["./Pesto"]);
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
}
