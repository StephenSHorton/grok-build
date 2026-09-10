//! Process identity that survives the fullscreen TUI.
//!
//! Official `grok` leaves `GROK_PROCESS_BRAND` unset. The Windows `grok-fork`
//! launcher sets it to `fork` so the tab title and welcome badge stay distinct
//! after alternate-screen clears the launch log.

use std::sync::OnceLock;

fn brand() -> &'static str {
    static BRAND: OnceLock<String> = OnceLock::new();
    BRAND.get_or_init(|| match std::env::var("GROK_PROCESS_BRAND") {
        Ok(v) if v.eq_ignore_ascii_case("fork") => "grok-fork".into(),
        _ => "grok".into(),
    })
}

/// Tab / window product name: `grok` or `grok-fork`.
pub fn product_name() -> &'static str {
    brand()
}

pub fn is_fork() -> bool {
    product_name() == "grok-fork"
}

/// Appended to the welcome version line, e.g. ` [fork]`.
pub fn fork_version_suffix() -> &'static str {
    if is_fork() { " [fork]" } else { "" }
}
