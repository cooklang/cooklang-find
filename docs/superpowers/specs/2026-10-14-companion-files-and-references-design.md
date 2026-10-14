# Companion files and recipe references

Date: 2026-10-14
Status: approved for planning
Release: 0.11.0

## Why

The iOS app re-implements what this library is for. It has three title-image
finders, three `.cook`/`.menu` checks, three resolvers for `@./path` references
and its own recipe counter, and they disagree with each other and with this
library on image extensions, letter case and which folder a reference is
relative to. Two of those disagreements are user-visible bugs:

- Step photos for recipes without sections never show in the app. The FFI
  reports `Name.N.jpg` as section 0, the app asks for section 1. Inside the
  library `Name.1.2.jpg` and `Name.2.jpg` are stored in the same slot, so the
  FFI cannot tell them apart even if the app asked correctly.
- Rename and delete in the app use different finders, so a `.webp`, `.JPG` or
  dotted-name title image moves with a rename but is left behind on delete.

The fix is to make this library the one place that knows the naming
conventions, and give the app small, pure functions it can call instead of
listing folders itself. The app migrates after this ships, as its own spec.

## Scope

In:

1. `companion_files` — the images that belong to a recipe file.
2. `renamed_companions` — the old/new path pairs when a recipe is renamed or
   moved.
3. `resolve_reference` — one rule for turning a `@./path` reference into a
   file, shared by `related_files` and `parse_menu`.
4. Step image storage that keeps linear and sectioned images apart, and an FFI
   that reports section 1 as 1.
5. One image extension list, matched in any letter case.
6. A corrected reference regex (names with spaces and dots).
7. The reference scanner shared between recipes and menus
   (`src/reference.rs`).
8. Docs, tests, 0.11.0 release.

Out:

- Performing renames, moves or deletes. The caller owns file I/O (the app
  coordinates iCloud writes and updates `.bookmarks` and `.shopping-list`).
- A collection/root object. The functions take `base_dir` explicitly like
  `list_menus_for_date` and `parse_menu` do today.
- Well-known collection files (`config/aisle.conf`, `.bookmarks`,
  `.shopping-list`). App-defined for now.
- Changing the spec repo. A separate small PR there fixes the `Name.0.jpg`
  example and lists the supported extensions.

## Design

### 1. `companion_files(recipe_path) -> Vec<Utf8PathBuf>`

The title image and every step and section image next to the recipe, as
paths in the recipe's folder, in the same form as the recipe path, in this order: title image first, then linear step images by
step, then sectioned images by section then step. Never a referenced recipe.

Rules:

- The recipe's stem is the file name without its last extension
  (`Dr.Cook.cook` → `Dr.Cook`). Companions are `stem.ext`, `stem.N.ext` and
  `stem.S.N.ext` with `N`, `S` ≥ 1.
- `ext` is matched case-insensitively against the shared list (§5). The
  returned path keeps the file's real spelling.
- One `read_dir` of the recipe's folder. No recipe file is opened.
- A recipe path with no directory component lists the current directory. A
  folder that cannot be read, or a path without a stem, yields an empty
  list. "No companions" is a normal state, not an error.
- `N` and `S` are plain decimals with no leading zero (`Name.01.jpg` is not a
  step image).
- The stem is compared exactly, byte for byte, on every filesystem:
  `pasta.jpg` is not a companion of `Pasta.cook` even where the filesystem
  is case-insensitive.
- If two files differ only by extension case or by extension
  (`Name.jpg` and `Name.PNG`), both are returned. `title_image()` keeps
  returning one: the first by the list order in §5, then by file name.

`title_image()` and `step_images()` are reworked onto the same directory scan
so the three never disagree again. The direct `glob()` call in
`recipe_entry.rs` goes; the `glob` crate stays, since `walk.rs`, the tree,
search and menu modules still use it. Likewise `regex` stays for
`menu/parse.rs` even though `recipe_entry.rs` stops using it.

FFI: `companion_files(recipe_path: String) -> Vec<String>` as a free function,
and `FfiRecipeEntry::companion_files()` for an entry loaded from a path
(empty for content-based entries).

### 2. `renamed_companions(from, to) -> Vec<RenamedFile>`

```rust
pub struct RenamedFile {
    pub from: Utf8PathBuf,
    pub to: Utf8PathBuf,
}
```

`from` is the recipe's current path, `to` its new path (possibly in another
folder; the extension may change between `.cook` and `.menu`). For every path
in `companion_files(from)`, the `to` path is the same file name with the old
stem replaced by the new stem, placed in `to`'s folder. The recipe file itself
is not in the list.

Pure path arithmetic after the one directory scan. Nothing at the destination
is checked, so a collision there is the caller's to detect, the same way the
caller already checks the recipe file itself. The caller is also responsible
for ordering (move the recipe, then the companions, or the reverse). A
companion whose new path equals its current path (same stem, same folder, as
in a `.cook` → `.menu` rename) is not listed, so the list is empty when
`from == to`.

FFI: `renamed_companions(from: String, to: String) -> Vec<FfiRenamedFile>`
with `FfiRenamedFile { from: String, to: String }`.

### 3. One reference rule: `resolve_reference(base_dir, reference) -> Option<Utf8PathBuf>`

`reference` is the raw path from a recipe or menu (`./sauces/Hollandaise`,
`../Pesto`, `./Sunday.menu`). It resolves against the collection root
`base_dir`, not the referencing file's folder. This is what CookCLI, the
`.shopping-list` format, `parse_menu` and every app call site already do;
`related_files` was the only outlier.

Algorithm:

1. Split on `/`; drop empty and `.` segments; `..` pops the previous segment.
   The result must not escape the root: if `..` would pop past the start,
   return `None`. `\` also separates, and a leading `/` is ignored (read as
   relative to the root).
2. Join to `base_dir`.
3. "Has an extension" means the last segment ends in `.cook` or `.menu`,
   compared case-insensitively like `walk::has_extension`. Any other dot is
   part of the name (`Dr.Cook`, `Mr. Smith's Stew`), which is what
   `menu/scale.rs` already does for `.menu`. With such a suffix, try the
   path as written first. Then, for every reference, try `<path>.cook` and
   `<path>.menu`: `Utf8Path::extension()` of `Dr.Cook` is `Cook`, so the
   fallback is what makes `./Dr.Cook` find `Dr.Cook.cook`. This
   also fixes `menu/scale.rs`, which today turns `./Pesto.cook` into
   `Pesto.cook.cook`; the menu scaling tests gain that case.
4. Return the first candidate that is an existing regular file, else `None`.

No canonicalisation and no symlink following: step 1 is the containment check,
so a symlink inside the root that points outside is the user's choice.

`related_files` becomes `related_files(&self, base_dir: &Utf8Path)` on
`RecipeEntry` and `related_files(base_dir: String)` on `FfiRecipeEntry`. It
uses `resolve_reference` for every reference and `companion_files` for images,
and keeps the recursion and cycle detection. Content-based entries still
return an empty list. `Menu::resolve_scales` keeps its `base_dirs` slice and
its FFI signature; it calls `resolve_reference` once per base dir, first hit
wins, so the two cannot drift. A menu reference that leaves the root is
treated as missing (raw multiplier), where 0.10 read the file from outside
the root.

Reference extraction: the regex `@(\.\.?/[^\s\{},.)]+)` in
`recipe_entry.rs` is replaced by the scanner the menu parser already has.
`braced` and `single_word_len` move from `src/menu/parse.rs` into a shared
`src/reference.rs` module behind one entry point,
`component_name(body) -> Option<(&str, usize)>`, which also owns the three
steps that live in `parse_component` today: rejecting `--` (a stray `@`
before a comment), stripping one leading modifier (`?`, `+`, `-`, `&`), and
skipping a trailing `(note)` after a braced component. The returned `usize`
is bytes consumed from `body`, modifier and note included. Both callers use
it, so a `@./path` on a line means the same file whether it sits in a
`.cook` or a `.menu`. Content is scanned line by line, as `scan_items`
does, so a name can never contain a newline. The rule, as the menu parser
has it today:

- Braced form: find the first `{` with a `}` after it; the name is
  everything before the `{`, trailing whitespace trimmed. If there is no
  closing `}`, or that text is empty, starts with whitespace, or contains
  `@`, `#`, `~`, `}` or `--`, it is not a braced component and the
  single-word rule applies instead. So `Add @./Pesto
  Sauce and @salt{}` yields `./Pesto`, because the text before `{` contains
  `@`.
- Single word: the name runs to the first whitespace or any of
  `,;:!?(){}@`, then trailing dots are trimmed. Dots inside the word stay:
  bare `@./Dr.Cook` yields `./Dr.Cook`, bare `@./Sunday.menu` yields
  `./Sunday.menu`, `Add @./Pesto.` yields `./Pesto`.

Only names starting with `./` or `../` are references. This single-word
rule is deliberately more lenient than cooklang-rs, whose lexer breaks a
word at `.` and `/` and so never accepts a braceless `@./Pesto`; an
implementer must not "fix" it to match the parser. The menu tests in
`menu/parse.rs` (`unbraced_recipe_reference` and neighbours) keep passing
unchanged, which is the check that the move preserved the rule.

Extraction skips YAML frontmatter and `[- ... -]` block comments and stops a
line at `--`, as the menu parser does, so a commented-out reference is not
followed. A fence line is `---` with trailing whitespace tolerated; an
unclosed fence means no frontmatter.

Cycle detection in `collect_related_files` keeps canonicalising paths for
its visited set; that is about not visiting one file twice, not about
where a reference points, so §3's "no canonicalisation" applies to
`resolve_reference` alone.

### 4. Step image storage

`StepImageCollection` stops using section index 0 to mean "linear":

```rust
pub struct StepImageCollection {
    /// `Name.N.ext`, keyed by one-based step.
    pub linear: BTreeMap<usize, String>,
    /// `Name.S.N.ext`, keyed by one-based section, then one-based step.
    pub sectioned: BTreeMap<usize, BTreeMap<usize, String>>,
}
```

`get(section, step)`: `section == 0` reads `linear`, otherwise `sectioned`.
`step == 0` is still `None`. `count()` and `is_empty()` cover both maps. Each
slot holds one path: when `Name.1.jpg` and `Name.1.png` both exist the
first by the §5 list order wins, the same tie-break as `title_image()`
(the existing `test_recipe_step_image_extension_priority` expects jpg).
`companion_files` returns both, in that same order. The struct derives
`Serialize`, so the JSON shape changes from `images` to `linear` and
`sectioned`; nothing in this repo reads it, but the release notes say so.

FFI `FfiStepImages` keeps its shape (`images: Vec<StepImageEntry>`, `count`)
but entries are now correct: linear images have `section: 0`, sectioned
images carry their real section, so `Name.1.2.jpg` reports `section: 1`.
`get_step_image(section, step)` follows the same rule. Order is linear first,
then by section and step; `BTreeMap` makes it deterministic.

This is a behaviour change for any caller that relied on section 1 arriving
as 0. The app is the only known caller and asks for section 1 today, so it
starts working rather than breaking.

### 5. Image extensions

One `pub const IMAGE_EXTENSIONS: [&str; 5] = ["jpg", "jpeg", "png", "webp",
"heic"]`, used by every image lookup, matched with `eq_ignore_ascii_case`.
`heic` is new: iPhone photos are saved in it. The order is the preference
order when a title image exists in several formats.

### 6. Hidden files and extension case

`companion_files` and the directory scan skip names starting with `.`, like
`list_dir` does. `.cook` and `.menu` already match in any case in `list_dir`;
`resolve_reference` compares extensions the same way.

## Public surface after this change

New:

- `cooklang_find::companion_files(recipe_path) -> Vec<Utf8PathBuf>`
- `cooklang_find::renamed_companions(from, to) -> Vec<RenamedFile>`
- `cooklang_find::resolve_reference(base_dir, reference) -> Option<Utf8PathBuf>`
- `cooklang_find::IMAGE_EXTENSIONS`
- `RecipeEntry::companion_files()`

`src/reference.rs` is a private module; `lib.rs` re-exports
`resolve_reference` (`pub use reference::resolve_reference`) next to the
existing `pub use model::*`. `companion_files`, `renamed_companions` and
`RenamedFile` live in `model` and come through that existing re-export.

Changed:

- `RecipeEntry::related_files(base_dir)` (was no-arg, recipe-folder relative)
- `StepImageCollection` fields (`linear`, `sectioned` replace `images`)
- FFI `FfiStepImages` entries report sectioned section numbers one-based
- `title_image()` matches any extension case and `heic`

FFI additions mirror the Rust ones: `companion_files`, `renamed_companions`,
`resolve_reference`, `FfiRecipeEntry::companion_files`,
`FfiRecipeEntry::related_files(base_dir)`, record `FfiRenamedFile`.

## Errors

None of the new functions return `Result`. A missing folder, an unreadable
folder, a recipe without a stem or a reference that escapes the root are all
answered with an empty list or `None`, because each is a legitimate state the
caller handles by showing nothing. A read failure inside a directory scan is
treated as "no entries"; the crate has no logging dependency, and the doc
comments say so, so callers that care check the folder themselves first.

## Tests

Rust unit tests in the module that owns each function, using `tempfile` as
the existing tests do:

Companions

- dotted recipe name (`Dr.Cook.cook` with `Dr.Cook.jpg`, `Dr.Cook.1.jpg`)
- mixed-case extensions (`Name.JPG`, `Name.2.Png`) are found and keep their
  spelling
- `heic` and `webp` found; `.gif` ignored
- a sibling with a longer stem (`Name Extra.jpg`) is not a companion
- hidden files skipped; missing folder gives an empty list
- order: title, linear by step, sectioned by section then step

Renamed companions

- rename in place, move to another folder, move with a new name, `.cook` to
  `.menu`
- recipe itself not included; empty when no companions; empty when
  `from == to`

Reference resolution

- `./Pesto` → `Pesto.cook`; `./Sunday` with only `Sunday.menu` → the menu;
  `./Sunday.menu` as written; `./Dr.Cook` → `Dr.Cook.cook`;
  `./Sunday.MENU` as written; `./Pesto.cook` → `Pesto.cook` (not
  `Pesto.cook.cook`), also through `parse_menu`
- `../` from the root and `./../..` return `None`
- `./a/../b` → `b.cook`
- a folder named like the reference is not a match
- extension case: `./Pesto` finds `Pesto.COOK`

Reference extraction

- `@./Pesto Sauce{100%g}` → `./Pesto Sauce`
- `@./Pesto Sauce` (no braces) → `./Pesto`
- `@./Dr.Cook{}` → `./Dr.Cook`; bare `@./Dr.Cook` → `./Dr.Cook`; bare
  `@./Sunday.menu` → `./Sunday.menu`; `Add @./Pesto.` → `./Pesto`
- `Add @./Pesto Sauce and @salt{}` → `./Pesto` only; `@./Pesto` at the end
  of a line followed by `then {` on the next line yields `./Pesto`, not a
  name with a newline in it
- `@-- comment` and `@?./Pesto{}` (modifier) behave as in the menu parser
- duplicates collapsed, order kept
- the existing `menu/parse.rs` tests pass unchanged after the move

Related files

- resolves from `base_dir` not the recipe's folder (a `sub/A.cook` referencing
  `./B` finds `B.cook` at the root, not `sub/B.cook`)
- recursion and cycles still handled; content entries return empty

Step images

- `Name.2.jpg` and `Name.1.2.jpg` both present: `get(0, 2)` and `get(1, 2)`
  return different files; FFI lists `section: 0` and `section: 1`
- `get(_, 0)` is `None`

FFI (`src/ffi.rs` tests, as the existing 16): each new export returns the
same as its Rust counterpart on a fixture folder.

## Docs and release

- README: new "Companion files" and "Resolving references" sections, and a
  new step image paragraph next to "Title Image Support" stating one-based
  numbering and section 0 for linear recipes.
- BINDINGS.md: list the new exports.
- Release notes on the GitHub release (the repo has no changelog file),
  calling out the `related_files` signature, the section-number change and
  the `StepImageCollection` JSON shape.
- Bump `Cargo.toml` to 0.11.0, tag `v0.11.0`; `release.yml` publishes the
  Swift package and the crate as it does today.
- Follow-ups outside this repo: a spec-repo PR for the README image section;
  the iOS app pins 0.11.0 and migrates under its own spec.
