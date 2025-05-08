pub mod fetcher;
pub mod model;
pub mod search;
pub mod tree;

pub use fetcher::get_recipe;
pub use model::*;
pub use search::search;
pub use tree::{build_tree, RecipeTree};
