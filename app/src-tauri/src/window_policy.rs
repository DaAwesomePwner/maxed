//! WebView2 launch policy; changing the saved preference never rebuilds a live window.

// Verified in wry 0.57.0 src/webview2/mod.rs. Supplying arguments replaces these defaults.
pub const DEFAULT_BROWSER_ARGS: &str =
    "--disable-features=msWebOOUI,msPdfOOUI,msSmartScreenProtection";

pub fn browser_args(hardware_acceleration: bool) -> String {
    if hardware_acceleration {
        DEFAULT_BROWSER_ARGS.to_owned()
    } else {
        format!("{DEFAULT_BROWSER_ARGS} --disable-gpu --in-process-gpu --enable-features=NetworkServiceInProcess2")
    }
}

pub fn measurement_demo(value: Option<&str>) -> bool {
    value == Some("1")
}

/// Optional repeat count; the original single-demo opt-in remains independent.
pub fn measurement_demo_loops(demo: Option<&str>, loops: Option<&str>) -> Option<u16> {
    if !measurement_demo(demo) {
        return None;
    }
    let text = loops?;
    if text.is_empty() || !text.bytes().all(|byte| byte.is_ascii_digit()) {
        return None;
    }
    text.parse::<u16>().ok().filter(|n| (1..=500).contains(n))
}
