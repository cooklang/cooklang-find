//! Line scanner that turns `.menu` content into a [`Menu`].

use super::model::MenuItem;

/// Appends the items found in `text` (one line, bullet and `\` already
/// removed) to `items`.
fn scan_items(text: &str, items: &mut Vec<MenuItem>) {
    let mut plain_start = 0;
    let mut i = 0;
    while i < text.len() {
        let rest = &text[i..];
        if let Some(note) = rest.strip_prefix("--") {
            push_text(&text[plain_start..i], items);
            let note = note.trim();
            if !note.is_empty() {
                items.push(MenuItem::Note {
                    text: note.to_string(),
                });
            }
            return;
        }
        if let Some(after_at) = rest.strip_prefix('@') {
            if let Some((item, len)) = parse_component(after_at) {
                push_text(&text[plain_start..i], items);
                items.push(item);
                i += 1 + len;
                plain_start = i;
                continue;
            }
        }
        i += rest.chars().next().map_or(1, char::len_utf8);
    }
    push_text(&text[plain_start..], items);
}

/// Pushes `text` as a [`MenuItem::Text`] unless it is blank.
fn push_text(text: &str, items: &mut Vec<MenuItem>) {
    if text.trim().is_empty() {
        return;
    }
    let mut collapsed = String::new();
    if text.starts_with(char::is_whitespace) {
        collapsed.push(' ');
    }
    collapsed.push_str(&text.split_whitespace().collect::<Vec<_>>().join(" "));
    if text.ends_with(char::is_whitespace) {
        collapsed.push(' ');
    }
    items.push(MenuItem::Text { text: collapsed });
}

/// Parses the component after an `@`, returning it and the bytes consumed.
fn parse_component(s: &str) -> Option<(MenuItem, usize)> {
    // Modifiers (`@?`, `@+`, `@-`, `@&`) don't change what is listed.
    let body = s.trim_start_matches(['?', '+', '-', '&']);
    let modifiers = s.len() - body.len();

    let (name, amount, len) = match braced(body) {
        Some((name, amount, mut len)) => {
            // Skip a trailing note such as `(cooked)`.
            if body[len..].starts_with('(') {
                if let Some(close) = body[len..].find(')') {
                    len += close + 1;
                }
            }
            (name, Some(amount), len)
        }
        None => {
            let len = single_word_len(body);
            if len == 0 {
                return None;
            }
            (&body[..len], None, len)
        }
    };

    let (quantity, unit) = split_amount(amount);
    let item = if name.starts_with("./") || name.starts_with("../") {
        recipe_reference(name, quantity, unit)
    } else {
        MenuItem::Ingredient {
            name: name.to_string(),
            quantity,
            unit,
        }
    };
    Some((item, modifiers + len))
}

/// Matches `name{amount}`, returning the name, the text inside the braces,
/// and the bytes consumed. The name may contain spaces but no other
/// component markers.
fn braced(body: &str) -> Option<(&str, &str, usize)> {
    let open = body.find('{')?;
    let name = &body[..open];
    if name.trim().is_empty()
        || name.starts_with(char::is_whitespace)
        || name.contains(['@', '#', '~', '}'])
        || name.contains("--")
    {
        return None;
    }
    let close = open + body[open..].find('}')?;
    Some((name.trim_end(), &body[open + 1..close], close + 1))
}

/// Length of an unbraced, single-word name.
fn single_word_len(body: &str) -> usize {
    let end = body
        .find(|c: char| c.is_whitespace() || ",;:!?(){}".contains(c))
        .unwrap_or(body.len());
    body[..end].trim_end_matches('.').len()
}

/// Splits `quantity%unit`, mapping blank parts to `None`.
fn split_amount(amount: Option<&str>) -> (Option<String>, Option<String>) {
    let Some(amount) = amount else {
        return (None, None);
    };
    let (quantity, unit) = match amount.split_once('%') {
        Some((quantity, unit)) => (quantity, Some(unit)),
        None => (amount, None),
    };
    (non_blank(quantity), unit.and_then(non_blank))
}

fn non_blank(s: &str) -> Option<String> {
    let s = s.trim();
    (!s.is_empty()).then(|| s.to_string())
}

fn recipe_reference(name: &str, quantity: Option<String>, unit: Option<String>) -> MenuItem {
    let path = name.strip_prefix("./").unwrap_or(name);
    let path = path.strip_suffix(".cook").unwrap_or(path);
    MenuItem::RecipeReference {
        name: path.rsplit('/').next().unwrap_or(path).to_string(),
        path: path.to_string(),
        quantity,
        unit,
        scale: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scan(text: &str) -> Vec<MenuItem> {
        let mut items = Vec::new();
        scan_items(text, &mut items);
        items
    }

    fn ingredient(name: &str, quantity: Option<&str>, unit: Option<&str>) -> MenuItem {
        MenuItem::Ingredient {
            name: name.to_string(),
            quantity: quantity.map(str::to_string),
            unit: unit.map(str::to_string),
        }
    }

    fn reference(path: &str, quantity: Option<&str>, unit: Option<&str>) -> MenuItem {
        MenuItem::RecipeReference {
            name: path.rsplit('/').next().unwrap().to_string(),
            path: path.to_string(),
            quantity: quantity.map(str::to_string),
            unit: unit.map(str::to_string),
            scale: None,
        }
    }

    fn text(t: &str) -> MenuItem {
        MenuItem::Text {
            text: t.to_string(),
        }
    }

    fn note(t: &str) -> MenuItem {
        MenuItem::Note {
            text: t.to_string(),
        }
    }

    #[test]
    fn ingredients_with_connecting_text() {
        assert_eq!(
            scan("@oats{1%cup} with @milk{1/2%cup}"),
            vec![
                ingredient("oats", Some("1"), Some("cup")),
                text(" with "),
                ingredient("milk", Some("1/2"), Some("cup")),
            ]
        );
    }

    #[test]
    fn multi_word_braced_ingredient() {
        assert_eq!(
            scan("@maple syrup{2%tbsp}"),
            vec![ingredient("maple syrup", Some("2"), Some("tbsp"))]
        );
    }

    #[test]
    fn unbraced_ingredient_is_one_word() {
        assert_eq!(
            scan("@salt and @black pepper{}"),
            vec![
                ingredient("salt", None, None),
                text(" and "),
                ingredient("black pepper", None, None),
            ]
        );
    }

    #[test]
    fn unbraced_ingredient_drops_trailing_period() {
        assert_eq!(scan("@eggs."), vec![ingredient("eggs", None, None), text(".")]);
    }

    #[test]
    fn quantity_without_unit() {
        assert_eq!(scan("@eggs{2}"), vec![ingredient("eggs", Some("2"), None)]);
    }

    #[test]
    fn trailing_note_in_parentheses_is_skipped() {
        assert_eq!(
            scan("@pasta{300%g}(cooked)"),
            vec![ingredient("pasta", Some("300"), Some("g"))]
        );
    }

    #[test]
    fn recipe_reference_strips_dot_slash_and_extension() {
        assert_eq!(
            scan("@./Breakfast/Shakshuka.cook{2}"),
            vec![reference("Breakfast/Shakshuka", Some("2"), None)]
        );
    }

    #[test]
    fn recipe_reference_with_spaces_and_servings() {
        assert_eq!(
            scan("@./Breakfast/Easy Pancakes{3%servings}"),
            vec![reference("Breakfast/Easy Pancakes", Some("3"), Some("servings"))]
        );
    }

    #[test]
    fn unbraced_recipe_reference() {
        assert_eq!(scan("@./lamb-chops"), vec![reference("lamb-chops", None, None)]);
    }

    #[test]
    fn parent_relative_reference_keeps_dot_dot() {
        assert_eq!(
            scan("@../Shared/Bread{}"),
            vec![reference("../Shared/Bread", None, None)]
        );
    }

    #[test]
    fn modifiers_are_ignored() {
        assert_eq!(
            scan("@?parsley{} @&./Sauce{}"),
            vec![
                ingredient("parsley", None, None),
                reference("Sauce", None, None),
            ]
        );
    }

    #[test]
    fn inline_comment_becomes_note() {
        assert_eq!(
            scan("@./Risotto{} -- use leftover stock"),
            vec![
                reference("Risotto", None, None),
                note("use leftover stock"),
            ]
        );
    }

    #[test]
    fn whole_line_comment_is_note() {
        assert_eq!(scan("-- save leftovers"), vec![note("save leftovers")]);
    }

    #[test]
    fn lone_at_sign_is_text() {
        assert_eq!(scan("meet @ noon"), vec![text("meet @ noon")]);
    }

    #[test]
    fn whitespace_collapses() {
        assert_eq!(scan("  a   b  "), vec![text(" a b ")]);
    }

    #[test]
    fn blank_text_is_dropped() {
        assert_eq!(scan("   "), vec![]);
    }
}
