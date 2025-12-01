//! # cooklang-find
//!
//! A library for finding, searching, and organizing Cooklang recipe files.
//!
//! This library provides utilities for working with .cook and .menu files,
//! including:
//! - Loading recipes from files or content
//! - Searching recipes by name and content
//! - Building hierarchical directory trees of recipes
//! - Extracting and working with recipe metadata
//!
//! ## Quick Start
//!
//! ```no_run
//! use cooklang_find::{get_recipe, search, build_tree};
//! use camino::Utf8Path;
//!
//! // Load a specific recipe
//! let recipe = get_recipe(vec!["./recipes"], "pancakes")?;
//!
//! // Search for recipes
//! let results = search(Utf8Path::new("./recipes"), "chocolate")?;
//!
//! // Build a directory tree
//! let tree = build_tree("./recipes")?;
//! # Ok::<(), Box<dyn std::error::Error>>(())
//! ```
//!
//! ## UniFFI Support
//!
//! This library includes UniFFI bindings for use on iOS and Android.
//! The FFI module provides simplified, FFI-safe types and functions.
//!
//! See the [`ffi`] module for FFI-specific types and functions.

// UniFFI scaffolding - must be at crate root
uniffi::setup_scaffolding!();

/// Recipe fetching utilities for loading recipes by name.
pub mod fetcher;

/// UniFFI bindings for cross-platform support (iOS, Android).
pub mod ffi;

/// Core data models for recipes and metadata.
pub mod model;

/// Recipe searching functionality.
pub mod search;

/// Recipe tree building for directory hierarchies.
pub mod tree;

pub use fetcher::{get_recipe, get_recipe_str};
pub use model::*;
pub use search::search;
pub use tree::{build_tree, RecipeTree};
