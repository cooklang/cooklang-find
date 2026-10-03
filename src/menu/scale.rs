//! Resolves `{target%unit}` on menu recipe references into multipliers,
//! per the Cooklang spec's "Scaling Referenced Recipes".

use super::model::{Menu, MenuItem};
use crate::fetcher::get_recipe;
use crate::model::Metadata;
use camino::Utf8Path;
use serde_yaml::Value;
use std::collections::HashMap;

impl Menu {
    /// Fills `scale` on every [`MenuItem::RecipeReference`].
    ///
    /// The reference's `{target%unit}` decides the multiplier:
    ///
    /// - nothing or a non-numeric target — 1
    /// - no unit — the target itself (`{2}` = ×2)
    /// - `servings` — target ÷ the referenced recipe's `servings`
    /// - any other unit — target ÷ the referenced recipe's `yield` value,
    ///   when the units match (case-insensitive)
    ///
    /// When the referenced recipe is missing or lacks the metadata, the
    /// target is used as a raw multiplier. The result is multiplied by
    /// `menu_scale`. Referenced recipes are looked up as `<path>.cook` in
    /// `base_dirs`.
    pub fn resolve_scales<P: AsRef<Utf8Path>>(&mut self, base_dirs: &[P], menu_scale: f64) {
        let mut sizes: HashMap<String, RecipeSize> = HashMap::new();
        let items = self
            .sections
            .iter_mut()
            .flat_map(|section| &mut section.meals)
            .flat_map(|meal| &mut meal.items);
        for item in items {
            if let MenuItem::RecipeReference {
                path,
                quantity,
                unit,
                scale,
                ..
            } = item
            {
                let size = sizes
                    .entry(path.clone())
                    .or_insert_with(|| RecipeSize::load(base_dirs, path));
                *scale =
                    Some(scale_factor(quantity.as_deref(), unit.as_deref(), size) * menu_scale);
            }
        }
    }
}

/// What a referenced recipe declares about its own size.
#[derive(Default)]
struct RecipeSize {
    servings: Option<f64>,
    yield_amount: Option<(f64, String)>,
}

impl RecipeSize {
    fn load<P: AsRef<Utf8Path>>(base_dirs: &[P], path: &str) -> Self {
        let file = format!("{path}.cook");
        match get_recipe(base_dirs.iter().map(|d| d.as_ref()), Utf8Path::new(&file)) {
            Ok(entry) => Self::from_metadata(entry.metadata()),
            Err(_) => Self::default(),
        }
    }

    fn from_metadata(metadata: &Metadata) -> Self {
        RecipeSize {
            servings: metadata.get("servings").and_then(value_number),
            yield_amount: metadata
                .get("yield")
                .and_then(Value::as_str)
                .and_then(parse_yield),
        }
    }
}

fn scale_factor(quantity: Option<&str>, unit: Option<&str>, size: &RecipeSize) -> f64 {
    let Some(target) = quantity.and_then(parse_number) else {
        return 1.0;
    };
    match unit {
        None => target,
        Some(unit)
            if unit.eq_ignore_ascii_case("servings") || unit.eq_ignore_ascii_case("serving") =>
        {
            match size.servings {
                Some(base) if base > 0.0 => target / base,
                _ => target,
            }
        }
        Some(unit) => match &size.yield_amount {
            Some((base, base_unit)) if base_unit.eq_ignore_ascii_case(unit) && *base > 0.0 => {
                target / base
            }
            _ => target,
        },
    }
}

/// Parses a Cooklang number: an optional leading `=` (fixed quantity)
/// followed by exactly one of an integer (`2`), a decimal (`1.5`), a
/// fraction (`1/2`), or a mixed number (`1 1/2`). Anything else (signs,
/// exponents, `inf`, `NaN`, ranges, several numbers) is rejected.
fn parse_number(s: &str) -> Option<f64> {
    let s = s.trim();
    let s = s.strip_prefix('=').unwrap_or(s).trim_start();
    let mut parts = s.split_whitespace();
    let first = parts.next()?;
    let value = match (parts.next(), parts.next()) {
        (None, _) => parse_fraction(first).or_else(|| parse_decimal(first))?,
        (Some(fraction), None) => parse_integer(first)? + parse_fraction(fraction)?,
        (Some(_), Some(_)) => return None,
    };
    value.is_finite().then_some(value)
}

fn parse_integer(s: &str) -> Option<f64> {
    if s.is_empty() || !s.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    s.parse().ok()
}

fn parse_decimal(s: &str) -> Option<f64> {
    match s.split_once('.') {
        Some((whole, fraction)) => {
            parse_integer(whole)?;
            parse_integer(fraction)?;
            s.parse().ok()
        }
        None => parse_integer(s),
    }
}

fn parse_fraction(s: &str) -> Option<f64> {
    let (numerator, denominator) = s.split_once('/')?;
    let numerator = parse_integer(numerator)?;
    let denominator = parse_integer(denominator)?;
    (denominator != 0.0).then(|| numerator / denominator)
}

fn value_number(value: &Value) -> Option<f64> {
    match value.as_f64() {
        Some(number) => number.is_finite().then_some(number),
        None => value.as_str().and_then(parse_number),
    }
}

/// Parses `yield` metadata of the form `VALUE%UNIT`, e.g. `500%ml`.
fn parse_yield(s: &str) -> Option<(f64, String)> {
    let (value, unit) = s.split_once('%')?;
    let unit = unit.trim();
    if unit.is_empty() {
        return None;
    }
    Some((parse_number(value)?, unit.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::menu::{Menu, MenuItem};
    use camino::Utf8PathBuf;
    use std::fs;
    use tempfile::TempDir;

    fn scales(menu: &Menu) -> Vec<Option<f64>> {
        menu.recipe_references()
            .into_iter()
            .map(|item| match item {
                MenuItem::RecipeReference { scale, .. } => *scale,
                _ => unreachable!(),
            })
            .collect()
    }

    fn recipes_dir() -> (TempDir, Utf8PathBuf) {
        let temp = TempDir::new().unwrap();
        let dir = Utf8PathBuf::from_path_buf(temp.path().to_path_buf()).unwrap();
        fs::write(
            dir.join("Pancakes.cook"),
            "---\nservings: 2\n---\n@flour{}\n",
        )
        .unwrap();
        fs::write(
            dir.join("Stock.cook"),
            "---\nyield: 500%ml\n---\n@bones{}\n",
        )
        .unwrap();
        (temp, dir)
    }

    fn resolve(line: &str, dir: &Utf8PathBuf, menu_scale: f64) -> Vec<Option<f64>> {
        let mut menu = Menu::parse(line, "m");
        menu.resolve_scales(&[dir], menu_scale);
        scales(&menu)
    }

    #[test]
    fn no_quantity_is_one() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{}", &dir, 1.0), vec![Some(1.0)]);
    }

    #[test]
    fn bare_number_is_raw_multiplier() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{3}", &dir, 1.0), vec![Some(3.0)]);
    }

    #[test]
    fn fraction_is_parsed() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{1/2}", &dir, 1.0), vec![Some(0.5)]);
    }

    #[test]
    fn servings_divide_by_recipe_servings() {
        let (_t, dir) = recipes_dir();
        assert_eq!(
            resolve("@./Pancakes{10%servings}", &dir, 1.0),
            vec![Some(5.0)]
        );
    }

    #[test]
    fn yield_with_matching_unit_divides() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Stock{1000%ML}", &dir, 1.0), vec![Some(2.0)]);
    }

    #[test]
    fn yield_with_other_unit_is_raw() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Stock{2%l}", &dir, 1.0), vec![Some(2.0)]);
    }

    #[test]
    fn missing_recipe_is_raw() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Nope{4%servings}", &dir, 1.0), vec![Some(4.0)]);
    }

    #[test]
    fn non_numeric_quantity_is_one() {
        let (_t, dir) = recipes_dir();
        assert_eq!(resolve("@./Pancakes{some}", &dir, 1.0), vec![Some(1.0)]);
    }

    #[test]
    fn menu_scale_multiplies() {
        let (_t, dir) = recipes_dir();
        assert_eq!(
            resolve("@./Pancakes{10%servings}", &dir, 2.0),
            vec![Some(10.0)]
        );
    }

    #[test]
    fn parse_number_accepts_cooklang_numbers() {
        let accepted = [
            ("2", 2.0),
            ("  2  ", 2.0),
            ("1.5", 1.5),
            ("0.5", 0.5),
            ("1/2", 0.5),
            ("3/4", 0.75),
            ("1 1/2", 1.5),
            ("2  3/4", 2.75),
            ("=2", 2.0),
            ("= 2", 2.0),
            ("=1 1/2", 1.5),
        ];
        for (input, expected) in accepted {
            assert_eq!(parse_number(input), Some(expected), "input {input:?}");
        }
    }

    #[test]
    fn parse_number_rejects_non_cooklang_numbers() {
        let rejected = [
            "",
            " ",
            "=",
            "inf",
            "NaN",
            "infinity",
            "-2",
            "+2",
            "1e2",
            "2 3",
            "1/2 1/2",
            "2-3",
            "1/0",
            "0/0",
            ".5",
            "5.",
            "1.5/2",
            "1/2.5",
            "1.5 1/2",
            "1 1/2 1/2",
            "==2",
            "2=",
            "1//2",
            "a",
            "2a",
            "１",
        ];
        for input in rejected {
            assert_eq!(parse_number(input), None, "input {input:?}");
        }
    }

    #[test]
    fn invalid_reference_quantities_are_one() {
        let (_t, dir) = recipes_dir();
        for quantity in ["2 3", "1e2", "-2", "inf", "NaN", "infinity%servings"] {
            assert_eq!(
                resolve(&format!("@./Pancakes{{{quantity}}}"), &dir, 1.0),
                vec![Some(1.0)],
                "quantity {quantity:?}"
            );
        }
    }

    #[test]
    fn non_finite_yield_is_absent() {
        let (_t, dir) = recipes_dir();
        fs::write(dir.join("Inf.cook"), "---\nyield: inf%ml\n---\n@x{}\n").unwrap();
        assert_eq!(resolve("@./Inf{1000%ml}", &dir, 1.0), vec![Some(1000.0)]);
    }
}
