//! Line scanner that turns `.menu` content into a [`Menu`].

use super::model::{Menu, MenuItem, MenuMeal, MenuSection};
use crate::model::{parse_yaml_content, Metadata};
use regex::Regex;
use std::sync::OnceLock;

impl Menu {
    /// Parses menu `content`.
    ///
    /// `fallback_name` is used when the frontmatter has no `title`, typically
    /// the file stem. Parsing never fails: content that isn't recognised is
    /// kept as [`MenuItem::Text`].
    ///
    /// # Examples
    ///
    /// ```
    /// use cooklang_find::Menu;
    ///
    /// let menu = Menu::parse("= Monday (2026-03-09)\nDinner:\n- @./Risotto{}\n", "Week");
    /// assert_eq!(menu.dates(), vec!["2026-03-09"]);
    /// ```
    pub fn parse(content: &str, fallback_name: &str) -> Menu {
        let (metadata, body) = split_frontmatter(content);
        let body = strip_block_comments(body);
        let name = metadata
            .title()
            .map(str::to_string)
            .unwrap_or_else(|| fallback_name.to_string());

        let mut builder = Builder::default();
        for line in body.lines() {
            builder.line(line);
        }

        Menu {
            name,
            metadata,
            sections: builder.finish(),
        }
    }
}

/// Splits leading `---` YAML frontmatter from the body.
fn split_frontmatter(content: &str) -> (Metadata, &str) {
    let mut lines = content.split_inclusive('\n');
    let Some(first) = lines.next() else {
        return (Metadata::default(), content);
    };
    if first.trim() != "---" {
        return (Metadata::default(), content);
    }
    let mut offset = first.len();
    for line in lines {
        if line.trim() == "---" {
            let metadata = parse_yaml_content(&content[first.len()..offset]).unwrap_or_default();
            return (metadata, &content[offset + line.len()..]);
        }
        offset += line.len();
    }
    (Metadata::default(), content)
}

fn strip_block_comments(body: &str) -> std::borrow::Cow<'_, str> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?s)\[-.*?-\]").unwrap())
        .replace_all(body, "")
}

fn extract_date(header: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{4}-\d{2}-\d{2}").unwrap())
        .find(header)
        .map(|m| m.as_str().to_string())
}

/// A `Breakfast (08:30):` heading and whatever follows it on the line.
struct MealHeader<'a> {
    meal_type: String,
    time: Option<String>,
    rest: &'a str,
}

impl<'a> MealHeader<'a> {
    fn parse(line: &'a str) -> Option<Self> {
        static RE: OnceLock<Regex> = OnceLock::new();
        let re = RE.get_or_init(|| {
            Regex::new(r"^([^@:()]+?)\s*(?:\((\d{1,2}:\d{2})\))?\s*:(?:\s|$)").unwrap()
        });
        if line.starts_with("--") {
            return None;
        }
        let caps = re.captures(line)?;
        let meal_type = caps[1].trim();
        if meal_type.is_empty() {
            return None;
        }
        Some(MealHeader {
            meal_type: meal_type.to_string(),
            time: caps.get(2).map(|m| m.as_str().to_string()),
            rest: &line[caps[0].len()..],
        })
    }
}

/// Accumulates sections and meals while lines are fed in.
#[derive(Default)]
struct Builder {
    sections: Vec<MenuSection>,
    section: MenuSection,
    meal: MenuMeal,
}

impl Builder {
    fn line(&mut self, raw: &str) {
        let trimmed = raw.trim();
        if trimmed.is_empty() {
            return;
        }
        if trimmed.starts_with('=') {
            self.start_section(trimmed.trim_matches('=').trim());
            return;
        }

        let line = trimmed.strip_suffix('\\').unwrap_or(trimmed).trim_end();
        let line = strip_bullet(line);
        match MealHeader::parse(line) {
            Some(header) => {
                self.flush_meal();
                self.meal.meal_type = Some(header.meal_type);
                self.meal.time = header.time;
                scan_items(header.rest, &mut self.meal.items);
            }
            None => scan_items(line, &mut self.meal.items),
        }
    }

    fn start_section(&mut self, name: &str) {
        self.flush_section();
        self.section = MenuSection {
            name: (!name.is_empty()).then(|| name.to_string()),
            date: extract_date(name),
            meals: Vec::new(),
        };
    }

    fn flush_meal(&mut self) {
        let meal = std::mem::take(&mut self.meal);
        if !meal.items.is_empty() {
            self.section.meals.push(meal);
        }
    }

    fn flush_section(&mut self) {
        self.flush_meal();
        let section = std::mem::take(&mut self.section);
        if section.name.is_some() || !section.meals.is_empty() {
            self.sections.push(section);
        }
    }

    fn finish(mut self) -> Vec<MenuSection> {
        self.flush_section();
        self.sections
    }
}

/// Removes a leading `- ` list bullet, leaving `--` comments intact.
fn strip_bullet(line: &str) -> &str {
    if line.starts_with("--") {
        return line;
    }
    line.strip_prefix('-').map_or(line, str::trim_start)
}

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

    use super::super::model::{Menu, MenuMeal, MenuSection};
    use indoc::indoc;

    fn meal(meal_type: Option<&str>, time: Option<&str>, items: Vec<MenuItem>) -> MenuMeal {
        MenuMeal {
            meal_type: meal_type.map(str::to_string),
            time: time.map(str::to_string),
            items,
        }
    }

    #[test]
    fn parses_weekly_plan() {
        // CookCLI seed/Weekly Plan.menu, first two days.
        let content = indoc! {r"
            ---
            servings: 2
            ---

            ==Saturday (2026-03-07)==

            Breakfast: \
            - @oats{1%cup} with @milk{1/2%cup} and @honey{1%tbsp}

            Lunch: \
            - @./lamb-chops{}

            -- save leftover lamb for sandwiches

            ==Sunday (2026-03-08)==

            Snacks: \
            - @almonds{50%g} \
            - @dark chocolate{30%g}
        "};

        let menu = Menu::parse(content, "Weekly Plan");

        assert_eq!(menu.name, "Weekly Plan");
        assert_eq!(menu.metadata.servings(), Some(2));
        assert_eq!(
            menu.sections,
            vec![
                MenuSection {
                    name: Some("Saturday (2026-03-07)".to_string()),
                    date: Some("2026-03-07".to_string()),
                    meals: vec![
                        meal(
                            Some("Breakfast"),
                            None,
                            vec![
                                ingredient("oats", Some("1"), Some("cup")),
                                text(" with "),
                                ingredient("milk", Some("1/2"), Some("cup")),
                                text(" and "),
                                ingredient("honey", Some("1"), Some("tbsp")),
                            ]
                        ),
                        meal(
                            Some("Lunch"),
                            None,
                            vec![
                                reference("lamb-chops", None, None),
                                note("save leftover lamb for sandwiches"),
                            ]
                        ),
                    ],
                },
                MenuSection {
                    name: Some("Sunday (2026-03-08)".to_string()),
                    date: Some("2026-03-08".to_string()),
                    meals: vec![meal(
                        Some("Snacks"),
                        None,
                        vec![
                            ingredient("almonds", Some("50"), Some("g")),
                            ingredient("dark chocolate", Some("30"), Some("g")),
                        ]
                    )],
                },
            ]
        );
    }

    #[test]
    fn title_overrides_fallback_name() {
        let menu = Menu::parse("---\ntitle: Summer Week\n---\n= Day 1\n", "file");

        assert_eq!(menu.name, "Summer Week");
    }

    #[test]
    fn section_without_date() {
        let menu = Menu::parse("== Day 1 ==\nDinner:\n- @./Soup{}\n", "m");

        assert_eq!(menu.sections[0].name.as_deref(), Some("Day 1"));
        assert_eq!(menu.sections[0].date, None);
    }

    #[test]
    fn bare_date_header() {
        let menu = Menu::parse("= 2026-06-24 Dinner\n@./Stew{}\n", "m");

        assert_eq!(menu.sections[0].date.as_deref(), Some("2026-06-24"));
        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![reference("Stew", None, None)])]
        );
    }

    #[test]
    fn empty_sections_are_kept() {
        let menu = Menu::parse("= Day 1\n= Day 2\n@eggs{}\n", "m");

        assert_eq!(menu.sections.len(), 2);
        assert!(menu.sections[0].meals.is_empty());
    }

    #[test]
    fn meal_header_with_time() {
        let menu = Menu::parse("= Day 1\nBreakfast (08:30):\n- @eggs{2}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Breakfast"),
                Some("08:30"),
                vec![ingredient("eggs", Some("2"), None)]
            )]
        );
    }

    #[test]
    fn meal_header_with_inline_items() {
        let menu = Menu::parse("= Day 1\nBreakfast: @eggs{2}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Breakfast"),
                None,
                vec![ingredient("eggs", Some("2"), None)]
            )]
        );
    }

    #[test]
    fn colon_without_following_space_is_not_a_header() {
        let menu = Menu::parse("= Day 1\nSee https://example.com\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![text("See https://example.com")])]
        );
    }

    #[test]
    fn note_with_colon_is_not_a_header() {
        let menu = Menu::parse("= Day 1\n-- tip: prep the night before\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![note("tip: prep the night before")])]
        );
    }

    #[test]
    fn empty_meals_are_dropped() {
        let menu = Menu::parse("= Day 1\nBreakfast:\nLunch:\n- @./Soup{}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(Some("Lunch"), None, vec![reference("Soup", None, None)])]
        );
    }

    #[test]
    fn content_before_first_section_is_unnamed_section() {
        let menu = Menu::parse("@./Soup{}\n= Day 1\n", "m");

        assert_eq!(menu.sections[0].name, None);
        assert_eq!(menu.sections[1].name.as_deref(), Some("Day 1"));
    }

    #[test]
    fn block_comments_are_removed() {
        let menu = Menu::parse("= Day 1\n[- @./Hidden{}\nstill hidden -]\n@./Shown{}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![reference("Shown", None, None)])]
        );
    }

    #[test]
    fn empty_content() {
        let menu = Menu::parse("", "m");

        assert_eq!(menu.name, "m");
        assert!(menu.sections.is_empty());
    }
}
