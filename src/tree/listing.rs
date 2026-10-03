//! Shallow, one-level directory listing.
//!
//! [`build_tree`](super::build_tree) opens and parses every recipe in a
//! subtree, which is too slow for a UI that only shows one folder at a time
//! (and, on iCloud, can trigger downloads of files nobody is looking at).
//! [`list_dir`] reads only the folder being shown: recipes directly inside it
//! are loaded, subfolders are only counted, by file name, never by content.
//! <https://github.com/cooklang/cooklang-find/issues/17>

use super::TreeError;
use crate::model::RecipeEntry;
use camino::{Utf8Path, Utf8PathBuf};
use std::fs;

/// The direct children of one directory.
#[derive(Debug)]
pub struct DirListing {
    /// The directory that was listed.
    pub path: Utf8PathBuf,
    /// Its recipes and subfolders, sorted by file name.
    pub entries: Vec<DirEntry>,
}

/// One child of a [`DirListing`].
#[derive(Debug)]
pub enum DirEntry {
    /// A `.cook`/`.menu` file directly in the listed directory. Its metadata
    /// is parsed; these are the only files [`list_dir`] opens.
    Recipe(RecipeEntry),
    /// A subdirectory. It is counted, never parsed.
    Folder {
        /// Directory name.
        name: String,
        /// Full path to the directory.
        path: Utf8PathBuf,
        /// Recipes in this folder, however deeply nested.
        recipe_count: usize,
    },
}

/// Lists the recipes and subfolders directly inside `dir`.
///
/// Only `.cook`/`.menu` files at this level are opened (via
/// [`RecipeEntry::from_path`]); a file whose content can't be read is left
/// out, as in [`build_tree`](super::build_tree). Each subfolder gets a
/// recursive [`count_recipes`], which looks at file names only. Folders
/// without any recipe are listed too, with a count of `0`, so a folder picker
/// can offer them.
///
/// Hidden files and directories (name starting with `.`) are skipped, as
/// they are by `build_tree`.
///
/// # Examples
///
/// ```no_run
/// use cooklang_find::tree::{list_dir, DirEntry};
///
/// let listing = list_dir("./recipes")?;
/// for entry in &listing.entries {
///     match entry {
///         DirEntry::Recipe(recipe) => println!("{:?}", recipe.name()),
///         DirEntry::Folder { name, recipe_count, .. } => {
///             println!("{name}/ ({recipe_count})")
///         }
///     }
/// }
/// # Ok::<(), Box<dyn std::error::Error>>(())
/// ```
pub fn list_dir<P: AsRef<Utf8Path>>(dir: P) -> Result<DirListing, TreeError> {
    let dir = dir.as_ref();
    check_dir(dir)?;

    let mut children = visible_children(dir)?;
    children.sort();

    let mut entries = Vec::new();
    for path in children {
        if path.is_dir() {
            entries.push(DirEntry::Folder {
                name: path.file_name().unwrap_or_default().to_string(),
                recipe_count: count_in(&path),
                path,
            });
        } else if is_recipe_file(&path) {
            if let Ok(recipe) = RecipeEntry::from_path(path) {
                entries.push(DirEntry::Recipe(recipe));
            }
        }
    }

    Ok(DirListing {
        path: dir.to_path_buf(),
        entries,
    })
}

/// Counts the `.cook`/`.menu` files under `dir`, however deeply nested.
///
/// Files are matched by name and never opened, so this is cheap and counts
/// files whose content isn't available locally (iCloud placeholders) too.
/// Hidden files and directories are skipped, as by
/// [`build_tree`](super::build_tree). Subdirectories that can't be read
/// contribute nothing rather than failing the count.
pub fn count_recipes<P: AsRef<Utf8Path>>(dir: P) -> Result<usize, TreeError> {
    let dir = dir.as_ref();
    check_dir(dir)?;
    Ok(count_in(dir))
}

fn check_dir(dir: &Utf8Path) -> Result<(), TreeError> {
    if !dir.exists() {
        return Err(TreeError::DirectoryNotFound(dir.to_string()));
    }
    if !dir.is_dir() {
        return Err(TreeError::NotADirectory(dir.to_string()));
    }
    Ok(())
}

fn count_in(dir: &Utf8Path) -> usize {
    let Ok(children) = visible_children(dir) else {
        return 0;
    };
    children
        .iter()
        .map(|path| {
            if path.is_dir() {
                count_in(path)
            } else {
                usize::from(is_recipe_file(path))
            }
        })
        .sum()
}

/// Paths of the entries of `dir`, without hidden ones and without names that
/// aren't valid UTF-8 (which `build_tree` can't represent either).
fn visible_children(dir: &Utf8Path) -> Result<Vec<Utf8PathBuf>, TreeError> {
    let mut paths = Vec::new();
    for entry in fs::read_dir(dir)? {
        let Ok(path) = Utf8PathBuf::from_path_buf(entry?.path()) else {
            continue;
        };
        if path.file_name().is_some_and(|n| !n.starts_with('.')) {
            paths.push(path);
        }
    }
    Ok(paths)
}

fn is_recipe_file(path: &Utf8Path) -> bool {
    matches!(path.extension(), Some("cook" | "menu"))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::build_tree;
    use indoc::indoc;
    use tempfile::TempDir;

    fn temp_dir() -> (TempDir, Utf8PathBuf) {
        let temp_dir = TempDir::new().unwrap();
        let path = Utf8PathBuf::from_path_buf(temp_dir.path().to_path_buf()).unwrap();
        (temp_dir, path)
    }

    fn create_recipe(dir: &Utf8Path, file_name: &str, content: &str) -> Utf8PathBuf {
        fs::create_dir_all(dir).unwrap();
        let path = dir.join(file_name);
        fs::write(&path, content).unwrap();
        path
    }

    fn folder<'a>(listing: &'a DirListing, wanted: &str) -> (&'a Utf8PathBuf, usize) {
        listing
            .entries
            .iter()
            .find_map(|e| match e {
                DirEntry::Folder {
                    name,
                    path,
                    recipe_count,
                } if name == wanted => Some((path, *recipe_count)),
                _ => None,
            })
            .unwrap_or_else(|| panic!("no folder {wanted}"))
    }

    fn recipe_names(listing: &DirListing) -> Vec<String> {
        listing
            .entries
            .iter()
            .filter_map(|e| match e {
                DirEntry::Recipe(r) => r.name().clone(),
                _ => None,
            })
            .collect()
    }

    #[test]
    fn lists_recipes_and_folders_one_level_deep() {
        let (_tmp, root) = temp_dir();
        create_recipe(
            &root,
            "pancakes.cook",
            indoc! {r#"
                ---
                title: Fluffy Pancakes
                ---

                Make pancakes"#},
        );
        create_recipe(&root, "week.menu", "Monday: @./pancakes{}");
        create_recipe(&root.join("dessert"), "cake.cook", "Bake cake");
        fs::write(root.join("notes.txt"), "not a recipe").unwrap();
        fs::write(root.join("pancakes.jpg"), "image").unwrap();

        let listing = list_dir(&root).unwrap();

        assert_eq!(listing.path, root);
        assert_eq!(listing.entries.len(), 3);
        assert_eq!(recipe_names(&listing), vec!["Fluffy Pancakes", "week"]);
        let (path, count) = folder(&listing, "dessert");
        assert_eq!(path, &root.join("dessert"));
        assert_eq!(count, 1);
    }

    #[test]
    fn recipe_entries_carry_title_image() {
        let (_tmp, root) = temp_dir();
        create_recipe(&root, "pancakes.cook", "Make pancakes");
        fs::write(root.join("pancakes.jpg"), "image").unwrap();

        let listing = list_dir(&root).unwrap();

        let DirEntry::Recipe(recipe) = &listing.entries[0] else {
            panic!("expected a recipe");
        };
        assert!(recipe.title_image().is_some());
    }

    #[test]
    fn folder_counts_are_recursive_and_match_build_tree() {
        let (_tmp, root) = temp_dir();
        let bakery = root.join("bakery");
        create_recipe(&bakery, "pita.cook", "Bake pita");
        create_recipe(&bakery.join("breads"), "sourdough.cook", "Bake sourdough");
        create_recipe(&bakery.join("breads"), "focaccia.cook", "Bake focaccia");
        create_recipe(&bakery.join("breads/old"), "rye.menu", "Bake rye");

        let listing = list_dir(&root).unwrap();
        let tree = build_tree(&root).unwrap();

        let (_, count) = folder(&listing, "bakery");
        assert_eq!(count, 4);
        assert_eq!(count, tree.children["bakery"].recipe_count());

        let nested = list_dir(&bakery).unwrap();
        let (_, breads) = folder(&nested, "breads");
        assert_eq!(
            breads,
            tree.children["bakery"].children["breads"].recipe_count()
        );
        assert_eq!(count_recipes(&root).unwrap(), tree.recipe_count());
    }

    #[test]
    fn folder_holding_only_subfolders() {
        let (_tmp, root) = temp_dir();
        let household = root.join("household");
        create_recipe(&household.join("laundry"), "detergent.cook", "Mix");
        create_recipe(&household.join("floors"), "cleaner.cook", "Mix");
        create_recipe(&household.join("floors"), "polish.cook", "Mix");

        let listing = list_dir(&household).unwrap();

        assert!(recipe_names(&listing).is_empty());
        assert_eq!(folder(&listing, "floors").1, 2);
        assert_eq!(folder(&listing, "laundry").1, 1);
        assert_eq!(folder(&list_dir(&root).unwrap(), "household").1, 3);
    }

    #[test]
    fn empty_folders_are_listed_with_zero_count() {
        let (_tmp, root) = temp_dir();
        fs::create_dir_all(root.join("empty/nested")).unwrap();

        let listing = list_dir(&root).unwrap();

        assert_eq!(folder(&listing, "empty").1, 0);
    }

    #[test]
    fn skips_hidden_files_and_directories() {
        let (_tmp, root) = temp_dir();
        create_recipe(&root, "pancakes.cook", "Make pancakes");
        fs::write(root.join("._pancakes.cook"), b"\x00\x05\x16\x07").unwrap();
        create_recipe(&root.join(".Trashes"), "deleted.cook", "Gone");
        create_recipe(&root.join("dessert"), "cake.cook", "Bake cake");
        fs::write(root.join("dessert/._cake.cook"), b"\x00\x05\x16\x07").unwrap();
        create_recipe(&root.join("dessert/.git"), "stale.cook", "Old");

        let listing = list_dir(&root).unwrap();

        assert_eq!(listing.entries.len(), 2);
        assert_eq!(recipe_names(&listing), vec!["pancakes"]);
        assert_eq!(folder(&listing, "dessert").1, 1);
        assert_eq!(
            count_recipes(&root).unwrap(),
            build_tree(&root).unwrap().recipe_count()
        );
    }

    #[cfg(unix)]
    #[test]
    fn counts_files_whose_content_cannot_be_read() {
        use std::os::unix::fs::PermissionsExt;

        let (_tmp, root) = temp_dir();
        let dessert = root.join("dessert");
        create_recipe(&dessert, "cake.cook", "Bake cake");
        let locked = create_recipe(&dessert, "pie.cook", "Bake pie");
        fs::set_permissions(&locked, fs::Permissions::from_mode(0o000)).unwrap();
        if fs::read(&locked).is_ok() {
            return; // Running as root: permissions don't stop the read.
        }

        // Neither file is opened to be counted.
        assert_eq!(folder(&list_dir(&root).unwrap(), "dessert").1, 2);
        // At the listed level, an unreadable recipe can't be loaded, so it is
        // left out of the entries, as build_tree does.
        assert_eq!(recipe_names(&list_dir(&dessert).unwrap()), vec!["cake"]);
    }

    #[test]
    fn reports_missing_and_non_directory_paths() {
        let (_tmp, root) = temp_dir();
        let file = create_recipe(&root, "pancakes.cook", "Make pancakes");

        assert!(matches!(
            list_dir(root.join("missing")),
            Err(TreeError::DirectoryNotFound(_))
        ));
        assert!(matches!(list_dir(&file), Err(TreeError::NotADirectory(_))));
        assert!(matches!(
            count_recipes(&file),
            Err(TreeError::NotADirectory(_))
        ));
    }
}
