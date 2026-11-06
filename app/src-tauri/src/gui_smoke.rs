//! Opt-in safety boundary for the debug GUI smoke harness.

pub fn requested(value: Option<&str>) -> bool {
    cfg!(debug_assertions) && value == Some("1")
}

/// Snapshot once, before SetupService construction and before any webview exists.
pub fn enabled() -> bool {
    #[cfg(debug_assertions)]
    {
        static ENABLED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *ENABLED.get_or_init(|| requested(std::env::var("MAXED_GUI_SMOKE").ok().as_deref()))
    }
    #[cfg(not(debug_assertions))]
    {
        requested(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_exact_opt_in_in_debug_builds() {
        assert_eq!(requested(Some("1")), cfg!(debug_assertions));
        for value in [
            None,
            Some(""),
            Some("0"),
            Some("true"),
            Some(" 1"),
            Some("1 "),
        ] {
            assert!(!requested(value));
        }
    }
}
