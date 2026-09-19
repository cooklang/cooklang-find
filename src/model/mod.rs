//! Core data models for recipes and metadata.
//!
//! This module contains the fundamental data structures used throughout
//! the library, including recipe entries and their associated metadata.

pub(crate) mod lossy;
mod metadata;
mod recipe_entry;

pub use metadata::Metadata;
pub use recipe_entry::{RecipeEntry, RecipeEntryError, StepImageCollection};

/// Crate-internal helpers shared with `search::filter`'s condition matcher,
/// reusing the same "list" and "candidate strings" interpretation of a YAML
/// value that `Metadata::tags` and `Metadata::get_path` already define.
pub(crate) use metadata::{value_as_list, value_candidate_strings};
