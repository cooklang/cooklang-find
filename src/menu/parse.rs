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
        let content = content.strip_prefix('\u{feff}').unwrap_or(content);
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

/// Returns the name of a `= Name` / `== Name ==` section header, with the
/// `=` markers and surrounding whitespace trimmed; `None` for other lines.
pub(super) fn section_name(line: &str) -> Option<&str> {
    let trimmed = line.trim();
    trimmed
        .starts_with('=')
        .then(|| trimmed.trim_matches('=').trim())
}

fn extract_date(header: &str) -> Option<String> {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?:^|\D)(\d{4}-\d{2}-\d{2})(?:\D|$)").unwrap())
        .captures(header)
        .map(|caps| caps[1].to_string())
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
            Regex::new(r"^([^@:]+?)\s*(?:\((\d{1,2}:\d{2})\))?\s*:(?:\s|$)").unwrap()
        });
        if line.starts_with("--") {
            return None;
        }
        let caps = re.captures(line)?;
        let meal_type = caps[1].trim();
        let rest = &line[caps[0].len()..];
        // `Tip: prep ahead` is text; a heading is followed by nothing, items,
        // or a `--` note.
        let after = rest.trim_start();
        let heading_follows = after.is_empty() || after.starts_with('@') || after.starts_with("--");
        if meal_type.is_empty() || !heading_follows {
            return None;
        }
        Some(MealHeader {
            meal_type: meal_type.to_string(),
            time: caps.get(2).map(|m| m.as_str().to_string()),
            rest,
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
        // Blank lines, `---` rules, and deprecated `>>` metadata carry no items.
        if trimmed.is_empty() || is_rule(trimmed) || trimmed.starts_with(">>") {
            return;
        }
        if let Some(name) = section_name(trimmed) {
            self.start_section(name);
            return;
        }

        let line = trimmed.strip_suffix('\\').unwrap_or(trimmed).trim_end();
        let line = strip_bullet(line);
        let rest = match MealHeader::parse(line) {
            Some(header) => {
                self.flush_meal();
                self.meal.meal_type = Some(header.meal_type);
                self.meal.time = header.time;
                header.rest
            }
            None => line,
        };
        self.push_line_items(rest);
    }

    /// Scans one line's items into the current meal, separating them from
    /// items of earlier lines with a [`MenuItem::LineBreak`].
    fn push_line_items(&mut self, text: &str) {
        let mut items = Vec::new();
        scan_items(text, &mut items);
        if items.is_empty() {
            return;
        }
        if !self.meal.items.is_empty() {
            self.meal.items.push(MenuItem::LineBreak);
        }
        self.meal.items.append(&mut items);
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

/// True for a line of three or more dashes, such as `---`.
fn is_rule(line: &str) -> bool {
    line.len() >= 3 && line.bytes().all(|b| b == b'-')
}

/// Removes a leading `- ` list bullet. A `-` must be followed by whitespace
/// or end the line, so `-5 degrees` and `--` comments are left intact.
fn strip_bullet(line: &str) -> &str {
    match line.strip_prefix('-') {
        Some(rest) if rest.is_empty() || rest.starts_with(char::is_whitespace) => rest.trim_start(),
        _ => line,
    }
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
    // `@-- note` is a stray `@` before a comment, not an `@-` modifier.
    if s.starts_with("--") {
        return None;
    }
    // Modifiers (`@?`, `@+`, `@-`, `@&`) don't change what is listed.
    let body = s.strip_prefix(['?', '+', '-', '&']).unwrap_or(s);
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
        recipe_reference(name, quantity, unit)?
    } else if name.is_empty() {
        return None;
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
        .find(|c: char| c.is_whitespace() || ",;:!?(){}@".contains(c))
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

/// Builds a reference from `./Path/Name.cook`; `None` if the name is empty.
fn recipe_reference(
    name: &str,
    quantity: Option<String>,
    unit: Option<String>,
) -> Option<MenuItem> {
    let path = name.strip_prefix("./").unwrap_or(name);
    let path = path.strip_suffix(".cook").unwrap_or(path);
    let name = path.rsplit('/').next().unwrap_or(path);
    if name.is_empty() {
        return None;
    }
    Some(MenuItem::RecipeReference {
        name: name.to_string(),
        path: path.to_string(),
        quantity,
        unit,
        scale: None,
    })
}

#[cfg(test)]
mod tests {
    use super::super::model::{Menu, MenuMeal, MenuSection};
    use super::*;
    use indoc::indoc;

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
        assert_eq!(
            scan("@eggs."),
            vec![ingredient("eggs", None, None), text(".")]
        );
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
            vec![reference(
                "Breakfast/Easy Pancakes",
                Some("3"),
                Some("servings")
            )]
        );
    }

    #[test]
    fn unbraced_recipe_reference() {
        assert_eq!(
            scan("@./lamb-chops"),
            vec![reference("lamb-chops", None, None)]
        );
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
            vec![reference("Risotto", None, None), note("use leftover stock"),]
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
                                MenuItem::LineBreak,
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
                            MenuItem::LineBreak,
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
            vec![meal(
                Some("Lunch"),
                None,
                vec![reference("Soup", None, None)]
            )]
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
        let menu = Menu::parse(
            "= Day 1\n[- @./Hidden{}\nstill hidden -]\n@./Shown{}\n",
            "m",
        );

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

    #[test]
    fn line_breaks_separate_lines_of_a_meal() {
        let menu = Menu::parse(
            "= Day 1\nBreakfast: @eggs{2}\n- @syrup{}\n\n- @coffee{}\n-- strong\n",
            "m",
        );

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Breakfast"),
                None,
                vec![
                    ingredient("eggs", Some("2"), None),
                    MenuItem::LineBreak,
                    ingredient("syrup", None, None),
                    MenuItem::LineBreak,
                    ingredient("coffee", None, None),
                    MenuItem::LineBreak,
                    note("strong"),
                ]
            )]
        );
    }

    #[test]
    fn no_line_break_for_single_line_or_new_meal() {
        let menu = Menu::parse("= Day 1\nBreakfast:\n- @eggs{}\nLunch: @soup{}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![
                meal(
                    Some("Breakfast"),
                    None,
                    vec![ingredient("eggs", None, None)]
                ),
                meal(Some("Lunch"), None, vec![ingredient("soup", None, None)]),
            ]
        );
    }

    #[test]
    fn meal_header_time_still_parsed() {
        let header = MealHeader::parse("Breakfast (08:30):").unwrap();

        assert_eq!(header.meal_type, "Breakfast");
        assert_eq!(header.time.as_deref(), Some("08:30"));
    }

    #[test]
    fn meal_header_keeps_non_time_parentheses() {
        let menu = Menu::parse("= Day 1\nLunch (packed):\n- @./Wrap{}\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Lunch (packed)"),
                None,
                vec![reference("Wrap", None, None)]
            )]
        );
    }

    #[test]
    fn label_followed_by_text_is_not_a_header() {
        let menu = Menu::parse(
            "= Day 1\nDinner:\n- @./Stew{}\nTip: prep the night before\n",
            "m",
        );

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Dinner"),
                None,
                vec![
                    reference("Stew", None, None),
                    MenuItem::LineBreak,
                    text("Tip: prep the night before"),
                ]
            )]
        );
    }

    #[test]
    fn dash_without_space_is_not_a_bullet() {
        let menu = Menu::parse("= Day 1\n-5 degrees\n- @eggs{}\n-\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                None,
                None,
                vec![
                    text("-5 degrees"),
                    MenuItem::LineBreak,
                    ingredient("eggs", None, None),
                ]
            )]
        );
    }

    #[test]
    fn dash_rule_lines_are_skipped() {
        let menu = Menu::parse("= Day 1\nDinner:\n- @./Stew{}\n---\n-----\n", "m");

        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Dinner"),
                None,
                vec![reference("Stew", None, None)]
            )]
        );
    }

    #[test]
    fn legacy_metadata_lines_are_skipped() {
        let menu = Menu::parse(">> servings: 2\n= Day 1\n>> note: x\n@./Stew{}\n", "m");

        assert_eq!(menu.sections.len(), 1);
        assert_eq!(
            menu.sections[0].meals,
            vec![meal(None, None, vec![reference("Stew", None, None)])]
        );
    }

    #[test]
    fn parses_three_day_plan() {
        // cook.md Plans/3 Day Plan IX.menu, Day 1 trimmed.
        let content = indoc! {"
            ---
            servings: 2
            ---

            ==Day 1==

            Breakfast:
            - @./Breakfast/Shakshuka.cook{2}
            - @crusty bread{4%slices}
            - @filter coffee{1%cup} and @tea{1%cup}

            Lunch:
            - @./Lunches/Spaghetti carbonara.cook{2}
            - @./Salads/Caprese.cook{2}
        "};

        let menu = Menu::parse(content, "3 Day Plan IX");

        assert_eq!(
            menu.sections,
            vec![MenuSection {
                name: Some("Day 1".to_string()),
                date: None,
                meals: vec![
                    meal(
                        Some("Breakfast"),
                        None,
                        vec![
                            reference("Breakfast/Shakshuka", Some("2"), None),
                            MenuItem::LineBreak,
                            ingredient("crusty bread", Some("4"), Some("slices")),
                            MenuItem::LineBreak,
                            ingredient("filter coffee", Some("1"), Some("cup")),
                            text(" and "),
                            ingredient("tea", Some("1"), Some("cup")),
                        ]
                    ),
                    meal(
                        Some("Lunch"),
                        None,
                        vec![
                            reference("Lunches/Spaghetti carbonara", Some("2"), None),
                            MenuItem::LineBreak,
                            reference("Salads/Caprese", Some("2"), None),
                        ]
                    ),
                ],
            }]
        );
    }

    #[test]
    fn utf8_ingredient_names() {
        assert_eq!(
            scan("@crème fraîche{2%tbsp} et @café{}"),
            vec![
                ingredient("crème fraîche", Some("2"), Some("tbsp")),
                text(" et "),
                ingredient("café", None, None),
            ]
        );
    }

    #[test]
    fn crlf_line_endings() {
        let menu = Menu::parse(
            "---\r\nservings: 2\r\n---\r\n= Day 1\r\nDinner:\r\n- @./Stew{}\r\n- @bread{}\r\n",
            "m",
        );

        assert_eq!(menu.metadata.servings(), Some(2));
        assert_eq!(menu.sections[0].name.as_deref(), Some("Day 1"));
        assert_eq!(
            menu.sections[0].meals,
            vec![meal(
                Some("Dinner"),
                None,
                vec![
                    reference("Stew", None, None),
                    MenuItem::LineBreak,
                    ingredient("bread", None, None),
                ]
            )]
        );
    }

    #[test]
    fn unclosed_brace_falls_back_to_single_word() {
        assert_eq!(
            scan("@salt{ and pepper"),
            vec![ingredient("salt", None, None), text("{ and pepper")]
        );
    }

    #[test]
    fn reference_amounts_with_unit_and_fraction() {
        assert_eq!(
            scan("@./Drinks/Lemonade{500%ml}"),
            vec![reference("Drinks/Lemonade", Some("500"), Some("ml"))]
        );
        assert_eq!(
            scan("@./Pie{1/2}"),
            vec![reference("Pie", Some("1/2"), None)]
        );
    }

    #[test]
    fn unbraced_name_stops_at_at_sign() {
        assert_eq!(
            scan("@a@b{}"),
            vec![ingredient("a", None, None), ingredient("b", None, None)]
        );
    }

    #[test]
    fn only_one_modifier_is_stripped() {
        assert_eq!(scan("@??x{}"), vec![ingredient("?x", None, None)]);
    }

    #[test]
    fn empty_reference_names_stay_text() {
        assert_eq!(scan("@./.cook{}"), vec![text("@./.cook{}")]);
        assert_eq!(scan("@./Breakfast/{}"), vec![text("@./Breakfast/{}")]);
    }

    #[test]
    fn date_must_not_be_inside_longer_digit_run() {
        assert_eq!(extract_date("= 12026-03-071"), None);
        assert_eq!(
            extract_date("Mon(2026-03-07)").as_deref(),
            Some("2026-03-07")
        );
        assert_eq!(extract_date("2026-03-07").as_deref(), Some("2026-03-07"));
    }

    #[test]
    fn leading_bom_is_ignored() {
        let menu = Menu::parse("\u{feff}---\ntitle: Week\n---\n= Day 1\n@eggs{}\n", "m");

        assert_eq!(menu.name, "Week");
        assert_eq!(menu.sections[0].name.as_deref(), Some("Day 1"));
    }

    #[test]
    fn meal_header_followed_by_note() {
        let menu = Menu::parse(
            indoc! {"
                Lunch:
                @./Salad{}
                Dinner: -- eating out
                @./Pie{}
            "},
            "m",
        );

        assert_eq!(
            menu.sections[0].meals,
            vec![
                meal(Some("Lunch"), None, vec![reference("Salad", None, None)]),
                meal(
                    Some("Dinner"),
                    None,
                    vec![
                        note("eating out"),
                        MenuItem::LineBreak,
                        reference("Pie", None, None),
                    ]
                ),
            ]
        );
    }

    #[test]
    fn at_followed_by_comment_is_not_an_ingredient() {
        assert_eq!(scan("@-- note"), vec![text("@"), note("note")]);
        assert_eq!(
            scan("@eggs{} @-- later"),
            vec![ingredient("eggs", None, None), text(" @"), note("later")]
        );
    }

    #[test]
    fn menu_reference_keeps_menu_suffix() {
        assert_eq!(
            scan("@./Weekly.menu{4%servings}"),
            vec![reference("Weekly.menu", Some("4"), Some("servings"))]
        );
    }

    #[test]
    fn section_name_trims_markers() {
        assert_eq!(section_name("  == Day 1 ==  "), Some("Day 1"));
        assert_eq!(
            section_name("= 2026-06-24 Dinner"),
            Some("2026-06-24 Dinner")
        );
        assert_eq!(section_name("Dinner:"), None);
    }
}
