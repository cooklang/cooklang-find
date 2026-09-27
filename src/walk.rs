//! The directory walk shared by every function that discovers recipe files.

/// Glob `pattern`, skipping hidden files and directories.
///
/// A wildcard never matches a leading `.`, so `**/*.cook` passes over macOS
/// AppleDouble companions (`._Pancakes.cook`, binary resource forks that
/// macOS writes next to every file it touches on a non-HFS share) and does
/// not descend into `.git`, `.Trashes` and the like. Literal components are
/// unaffected: a collection that itself lives under `~/.recipes` is still
/// walked. <https://github.com/cooklang/cookcli/issues/555>
pub(crate) fn glob_visible(pattern: &str) -> Result<glob::Paths, glob::PatternError> {
    glob::glob_with(
        pattern,
        glob::MatchOptions {
            require_literal_leading_dot: true,
            ..Default::default()
        },
    )
}
