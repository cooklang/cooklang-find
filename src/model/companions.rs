//! The files that belong to a recipe by naming convention, found by one
//! listing of its folder:
//!
//! - `Name.ext` — the title image
//! - `Name.N.ext` — step N of a recipe without sections
//! - `Name.S.N.ext` — section S, step N
//!
//! `N` and `S` are one-based plain decimals, no leading zero. `ext` is one
//! of [`IMAGE_EXTENSIONS`], in any ASCII case. `Name` is the recipe file's
//! stem, so `Dr.Cook.cook` owns `Dr.Cook.jpg`. Referenced recipes are never companions.

use camino::{Utf8Path, Utf8PathBuf};
use serde::Serialize;
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
/// Files whose names start with `.` are skipped. A path without a stem, or
/// whose folder cannot be read, gives an empty list.
pub(crate) fn scan(recipe_path: &Utf8Path) -> Vec<(Slot, Utf8PathBuf)> {
    let (Some(dir), Some(stem)) = (recipe_path.parent(), recipe_path.file_stem()) else {
        return Vec::new();
    };
    let listed = if dir.as_str().is_empty() {
        Utf8Path::new(".")
    } else {
        dir
    };
    let Ok(entries) = fs::read_dir(listed) else {
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
            let numbers: Vec<usize> = numbers.split('.').map(step_number).collect::<Option<_>>()?;
            match numbers[..] {
                [step] => Slot::Linear(step),
                [section, step] => Slot::Sectioned(section, step),
                _ => return None,
            }
        }
    };
    Some((slot, rank))
}

/// A one-based step or section number: ASCII digits only, no leading zero.
fn step_number(text: &str) -> Option<usize> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) || text.starts_with('0') {
        return None;
    }
    text.parse().ok()
}

/// The title image and every step and section image next to
/// `recipe_path`, as paths in the recipe's folder, in the same form as
/// `recipe_path`: title first, then linear steps by step, then sectioned
/// images by section and step. Several files for one slot (`Name.jpg` and
/// `Name.PNG`) are all returned, in [`IMAGE_EXTENSIONS`] order. A recipe
/// this one references (`@./Other{}`) is not a companion; see
/// [`resolve_reference`](crate::resolve_reference). Opens no file; a path
/// without a stem, or whose folder cannot be read, gives an empty list.
pub fn companion_files(recipe_path: &Utf8Path) -> Vec<Utf8PathBuf> {
    scan(recipe_path)
        .into_iter()
        .map(|(_, path)| path)
        .collect()
}

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
        assert_eq!(
            classify("Dr.Cook.1.jpg", "Dr.Cook"),
            Some((Slot::Linear(1), 0))
        );
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
        assert_eq!(classify("Pasta.01.jpg", "Pasta"), None);
        assert_eq!(classify("Pasta.+1.jpg", "Pasta"), None);
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
            scan(&recipe).iter().map(|(s, _)| *s).collect::<Vec<_>>(),
            vec![
                Slot::Title,
                Slot::Title,
                Slot::Linear(1),
                Slot::Linear(2),
                Slot::Sectioned(1, 2),
                Slot::Sectioned(2, 1),
            ]
        );
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

    static CWD: std::sync::Mutex<()> = std::sync::Mutex::new(());

    #[test]
    fn bare_file_name_lists_the_current_directory() {
        let _guard = CWD.lock().unwrap_or_else(|e| e.into_inner());
        let (_t, dir) = root();
        touch(&dir, "Pasta.cook");
        touch(&dir, "Pasta.jpg");
        let previous = std::env::current_dir().unwrap();
        std::env::set_current_dir(&dir).unwrap();
        let found = companion_files(Utf8Path::new("Pasta.cook"));
        std::env::set_current_dir(previous).unwrap();
        assert_eq!(names(&found), vec!["Pasta.jpg"]);
        assert_eq!(found[0], Utf8Path::new("Pasta.jpg"));
    }

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
}
