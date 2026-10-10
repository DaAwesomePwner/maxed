pub fn valid_counter_step(before: i32, after: i32, delay_micros: u64) -> bool {
    before >= 0 && i64::from(after) - i64::from(before) == 1 && delay_micros <= 30_000_000
}

/// RealmShark CompletionDialogue.evidence: exact map, NPC and punctuation.
pub fn completion_rule(map: &str, npc: &str, text: &str) -> Option<&'static str> {
    match (map, npc, text) {
        ("Moonlight Village", "#Kitsune Umi", "This fully concludes the Moonlight Festival!") => {
            Some("moonlight-kitsune")
        }
        ("Moonlight Village", "#Dancer Miko", "Thank you all for coming tonight.") => {
            Some("moonlight-miko")
        }
        (
            "Moonlight Village",
            "#Umi, Goddess of Revelry",
            "This fully concludes the Moonlight Festival.",
        ) => Some("moonlight-revelry"),
        (
            "The Void",
            "#Void Entity",
            "You fools... You can never truly defeat me! I am in all of you! I AM all of you!",
        ) => Some("void-entity"),
        (
            "The Shatters",
            "#The Accursed King",
            "...do you truly think your end will be any different?",
        ) => Some("shatters-king"),
        ("The Shatters", "#King Azamoth", "This fate is mine to bear... not hers.") => {
            Some("shatters-azamoth")
        }
        _ => None,
    }
}
