//! Player-name presentation and matching; identity keys retain their raw inputs.

/// Removes the `,suffix` the client appends to player names.
pub fn display_name(raw: &str) -> &str {
    raw.split(',').next().unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn party_name_key_agrees_with_display_name() {
        for raw in [
            "SynthAlpha",
            "SYNTHalpha,fe14",
            "SynthBravo,fe14,extra",
            ",fe14",
            "",
        ] {
            assert_eq!(
                maxed_types::party::name_key(raw),
                display_name(raw).to_ascii_lowercase()
            );
        }
    }

    #[test]
    fn display_name_uses_everything_before_the_first_comma() {
        for (raw, expected) in [
            ("SynthAlpha", "SynthAlpha"),
            ("SynthAlpha,fe14", "SynthAlpha"),
            ("SynthAlpha,fe14,extra", "SynthAlpha"),
            ("", ""),
            (",fe14", ""),
        ] {
            assert_eq!(display_name(raw), expected);
        }
    }
}
