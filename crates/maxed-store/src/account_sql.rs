//! Static SQL slots reproduce migration 017 ownership without materializing joined views.
//! Slot arguments are repository SQL expressions, never user input; values remain bound.
const PREFIX: &str = "/*account:";

pub fn with_accounts(sql: &str) -> String {
    let mut result = sql.replace(
        "/*unattributed_history*/",
        include_str!("unattributed_history.sql"),
    );
    while let Some(start) = result.find(PREFIX) {
        let content = start + PREFIX.len();
        let end = content + result[content..].find("*/").expect("closed account slot");
        let (kind, value) = result[content..end]
            .split_once(':')
            .expect("account slot argument");
        let character = match kind {
            "character" => value.to_owned(),
            "run" => format!("(SELECT character_id FROM runs WHERE id={value})"),
            "fight" | "drop" => format!("coalesce((SELECT character_id FROM observation_characters WHERE kind='{kind}' AND observation_id={value}.id),(SELECT character_id FROM runs WHERE id={value}.run_id))"),
            _ => panic!("unknown account slot: {kind}"),
        };
        let expression = include_str!("account_character.sql")
            .trim()
            .replace("$character", &character);
        result.replace_range(start..end + 2, &expression);
    }
    result
}
