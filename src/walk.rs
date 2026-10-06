//! The directory walk shared by every function that discovers recipe files.

use camino::Utf8Path;

/// Glob `pattern`, skipping hidden files and directories.
///
/// A wildcard never matches a leading `.`, so `**/*.cook` passes over macOS
/// AppleDouble companions (`._Pancakes.cook`, binary resource forks that
/// macOS writes next to every file it touches on a non-HFS share) and does
/// not descend into `.git`, `.Trashes` and the like. Literal components are
/// unaffected: a collection that itself lives under `~/.recipes` is still
/// walked. <https://github.com/cooklang/cookcli/issues/555>
///
/// Wildcard components match case-insensitively, so `**/*.cook` finds
/// `Pancakes.COOK` on any filesystem. Literal components (the base dir) are
/// joined as given, not matched.
/// <https://github.com/cooklang/cooklang-find/issues/11>
pub(crate) fn glob_visible(pattern: &str) -> Result<glob::Paths, glob::PatternError> {
    glob::glob_with(
        pattern,
        glob::MatchOptions {
            case_sensitive: false,
            require_literal_separator: false,
            require_literal_leading_dot: true,
        },
    )
}

/// Whether `path` has extension `ext`, ignoring ASCII case (`x.COOK` is a
/// `cook` file). <https://github.com/cooklang/cooklang-find/issues/11>
pub(crate) fn has_extension(path: &Utf8Path, ext: &str) -> bool {
    path.extension()
        .is_some_and(|e| e.eq_ignore_ascii_case(ext))
}

/// Whether `path` names a `.cook` or `.menu` file, ignoring case.
pub(crate) fn is_recipe_file(path: &Utf8Path) -> bool {
    has_extension(path, "cook") || has_extension(path, "menu")
}
