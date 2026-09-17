//! Process identity that survives the fullscreen TUI.
//!
//! Official `grok` leaves `GROK_PROCESS_BRAND` unset. The `grok-fork`
//! launcher sets it to `fork` so the tab title and welcome badge stay distinct
//! after alternate-screen clears the launch log.
//!
//! Fork updates are the launcher (git rebase onto xai-org/grok-build + cargo
//! rebuild), not the CDN auto-updater. The branded process refuses
//! `--no-auto-update` / `GROK_DISABLE_AUTOUPDATER` and will not start without
//! `GROK_FORK_SYNC_OK` from a successful launcher pass.

use std::sync::OnceLock;

fn brand() -> &'static str {
    static BRAND: OnceLock<String> = OnceLock::new();
    BRAND.get_or_init(|| {
        if std::env::var("GROK_PROCESS_BRAND")
            .map(|v| v.eq_ignore_ascii_case("fork"))
            .unwrap_or(false)
        {
            return "grok-fork".into();
        }
        // Installing the binary as `grok-fork` (no wrapper) should still brand as fork.
        let argv0 = std::env::args_os().next();
        let name = argv0
            .as_ref()
            .and_then(|p| std::path::Path::new(p).file_stem())
            .and_then(|s| s.to_str())
            .unwrap_or("");
        if name.eq_ignore_ascii_case("grok-fork") {
            "grok-fork".into()
        } else {
            "grok".into()
        }
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
