//! Core data models for recipes and metadata.
//!
//! This module contains the fundamental data structures used throughout
//! the library, including recipe entries and their associated metadata.

mod metadata;
mod recipe_entry;

pub use metadata::Metadata;
pub use recipe_entry::{RecipeEntry, RecipeEntryError};
