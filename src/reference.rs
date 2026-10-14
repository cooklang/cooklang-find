//! Recipe references (`@./path`), shared by `.cook` and `.menu` handling:
//! the component scanner, reference extraction and reference resolution.
//!
//! The scanner is the menu parser's rule, used for recipe content too so a
//! `@./path` on a line means the same file in either kind of file. The
//! single-word fallback is deliberately more lenient than cooklang-rs, whose
//! lexer breaks a word at `.` and `/` and so never accepts a braceless
//! `@./Pesto`; do not "fix" it to match the parser.

use crate::walk::has_extension;
use camino::{Utf8Path, Utf8PathBuf};
use regex::Regex;
use std::borrow::Cow;
use std::collections::HashSet;
use std::fs;
use std::sync::OnceLock;

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

/// Removes `[- ... -]` block comments, which may span lines.
pub(crate) fn strip_block_comments(body: &str) -> Cow<'_, str> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)\[-.*?-\]").unwrap())
        .replace_all(body, "")
}

/// The content after an optional BOM and an optional leading `---` YAML
/// frontmatter block: the text after the closing `---` line, or an empty
/// string when the block is never closed. Content without frontmatter is
/// returned unchanged.
fn skip_frontmatter(content: &str) -> &str {
    let content = content.strip_prefix('\u{feff}').unwrap_or(content);
    let is_fence = |line: &str| line.trim_end_matches(['\r', '\n']) == "---";
    let mut lines = content.split_inclusive('\n');
    let Some(first) = lines.next().filter(|first| is_fence(first)) else {
        return content;
    };
    let mut offset = first.len();
    for line in lines {
        offset += line.len();
        if is_fence(line) {
            return &content[offset..];
        }
    }
    ""
}

/// The recipe references in `content`, in order of first appearance,
/// deduplicated, as written (`./sauces/Hollandaise`, `../Pesto`).
///
/// Scans line by line with [`component`], so a name never spans lines.
/// Frontmatter and `[- ... -]` block comments are skipped, and a line stops
/// at `--` (a comment).
pub(crate) fn extract_references(content: &str) -> Vec<String> {
    let body = strip_block_comments(skip_frontmatter(content));
    let mut seen = HashSet::new();
    let mut refs = Vec::new();
    for line in body.lines() {
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
/// Symlinks are not followed for the containment check. `\` is treated as a
/// separator too, so a Windows-style reference cannot smuggle `..` past the
/// check; a name containing `\` is not addressable. A leading `/` is
/// ignored, so an absolute path is read as relative to the root.
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
    if !dir.starts_with(base_dir) {
        return None;
    }

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
            .filter(|have| same_file_name(have, want))
            .map(|have| dir.join(have))
            .find(|path| path.is_file())
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

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

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
        assert_eq!(
            extract_references("Add @./Pesto Sauce and @salt{}"),
            vec!["./Pesto"]
        );
    }

    #[test]
    fn multi_word_and_dotted_names() {
        assert_eq!(
            extract_references("@./Pesto Sauce{100%g} @./Dr.Cook{} @./Sunday.menu"),
            vec!["./Pesto Sauce", "./Dr.Cook", "./Sunday.menu"]
        );
        assert_eq!(
            extract_references("@./Crème brûlée{}\r\n@./Dr.Cook"),
            vec!["./Crème brûlée", "./Dr.Cook"]
        );
    }

    #[test]
    fn scans_line_by_line() {
        // A brace on the next line never joins the name on this one.
        assert_eq!(extract_references("@./Pesto\nthen {stir}"), vec!["./Pesto"]);
    }

    #[test]
    fn comments_are_skipped() {
        assert_eq!(
            extract_references("@./Pesto{} -- was @./Old{}\n@-- not a modifier"),
            vec!["./Pesto"]
        );
    }

    #[test]
    fn frontmatter_and_block_comments_are_not_scanned() {
        assert_eq!(
            extract_references("---\ntitle: @./X\n---\n@./Y{}"),
            vec!["./Y"]
        );
        assert_eq!(
            extract_references("[- @./Hidden{} -] @./Shown"),
            vec!["./Shown"]
        );
        assert_eq!(
            extract_references("@./A{}\n[- many\nlines @./B{}\n-] @./C{}"),
            vec!["./A", "./C"]
        );
        assert_eq!(extract_references("\u{feff}---\n---\n@./Z{}"), vec!["./Z"]);
        assert_eq!(
            extract_references("---\nno closing @./X{}"),
            Vec::<String>::new()
        );
    }

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
        assert_eq!(
            resolve_reference(&dir, "./sauces/Pesto"),
            Some(pesto.clone())
        );
        assert_eq!(resolve_reference(&dir, "sauces/Pesto"), Some(pesto));
        assert_eq!(resolve_reference(&dir, "./Sunday"), Some(sunday.clone()));
        assert_eq!(
            resolve_reference(&dir, "./Sunday.menu"),
            Some(sunday.clone())
        );
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

    #[test]
    fn drive_letter_segment_cannot_escape_the_root() {
        let (_t, dir) = root();
        assert_eq!(resolve_reference(&dir, "./C:/Users/x/secrets"), None);
        assert_eq!(resolve_reference(&dir, "C:\\secrets"), None);
    }
}
