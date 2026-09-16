//! Suzuri host-pane split: emit OSC 7880 so the terminal splits and launches
//! a Grok process in a new PTY.
//!
//! Protocol (values percent-encoded):
//! - `/fork`: `ESC]7880;fork=1;resume=…;cwd=…;bin=…;prompt=…;title=…;brand=…BEL`
//! - new session: `ESC]7880;new=1;session=…;cwd=…;bin=…;prompt=…;title=…;brand=…BEL`

use std::io::Write;
use std::path::PathBuf;

/// Whether a suzuri pane advertised OSC 7880 support via env.
pub fn env_available() -> bool {
    env_flag("SUZURI_FORK_SPLIT") || env_flag("SUZURI")
}

/// Title the child pane/session should keep (`GROK_SESSION_TITLE` from suzuri).
pub fn env_session_title() -> Option<String> {
    let raw = std::env::var("GROK_SESSION_TITLE").ok()?;
    xai_grok_shell::session::persistence::sanitize_and_cap_title(&raw)
}

fn env_flag(name: &str) -> bool {
    std::env::var(name).ok().is_some_and(|v| {
        matches!(
            v.trim().to_ascii_lowercase().as_str(),
            "1" | "true" | "yes" | "on"
        )
    })
}

/// `auto` (default): split when the host advertised support.
/// `never`: keep in-process `/fork` even inside suzuri.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ForkHostSplitMode {
    #[default]
    Auto,
    Never,
}

impl ForkHostSplitMode {
    pub fn from_config_str(s: &str) -> Self {
        match s.trim().to_ascii_lowercase().as_str() {
            "never" | "off" | "false" | "0" => Self::Never,
            _ => Self::Auto,
        }
    }
}

/// Resolve whether this `/fork` should ask the host to split.
/// `override`: `Some(true)` from `--split`, `Some(false)` from `--no-split`.
pub fn should_host_split(
    available: bool,
    mode: ForkHostSplitMode,
    override_flag: Option<bool>,
) -> bool {
    match override_flag {
        Some(true) => available,
        Some(false) => false,
        None => available && mode != ForkHostSplitMode::Never,
    }
}

/// Fields written into OSC 7880. The host allowlists `bin` and builds argv.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostForkSplit {
    pub resume: String,
    pub cwd: PathBuf,
    pub bin: PathBuf,
    pub prompt: Option<String>,
    pub title: Option<String>,
    pub brand_fork: bool,
    /// `true` → `new=1;session=` (`--session-id`). `false` → `fork=1;resume=` (`--resume`).
    pub new_session: bool,
}

impl HostForkSplit {
    pub fn for_child_session(
        resume: impl Into<String>,
        cwd: impl Into<PathBuf>,
        prompt: Option<String>,
    ) -> Option<Self> {
        let bin = std::env::current_exe().ok()?;
        Some(Self {
            resume: resume.into(),
            cwd: cwd.into(),
            bin,
            prompt,
            title: None,
            brand_fork: crate::brand::is_fork(),
            new_session: false,
        })
    }

    pub fn for_new_session(
        session_id: impl Into<String>,
        cwd: impl Into<PathBuf>,
        prompt: Option<String>,
        title: Option<String>,
    ) -> Option<Self> {
        let bin = std::env::current_exe().ok()?;
        Some(Self {
            resume: session_id.into(),
            cwd: cwd.into(),
            bin,
            prompt,
            title,
            brand_fork: crate::brand::is_fork(),
            new_session: true,
        })
    }
}

pub fn encode_osc(req: &HostForkSplit) -> Vec<u8> {
    let mut s = if req.new_session {
        String::from("\x1b]7880;new=1")
    } else {
        String::from("\x1b]7880;fork=1")
    };
    if req.new_session {
        push_kv(&mut s, "session", &req.resume);
    } else {
        push_kv(&mut s, "resume", &req.resume);
    }
    push_kv(&mut s, "cwd", &req.cwd.to_string_lossy());
    push_kv(&mut s, "bin", &req.bin.to_string_lossy());
    if let Some(p) = req
        .prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
    {
        push_kv(&mut s, "prompt", p);
    }
    if let Some(t) = req
        .title
        .as_deref()
        .map(str::trim)
        .filter(|t| !t.is_empty())
    {
        push_kv(&mut s, "title", t);
    }
    if req.brand_fork {
        push_kv(&mut s, "brand", "fork");
    }
    s.push('\x07');
    s.into_bytes()
}

fn push_kv(s: &mut String, key: &str, val: &str) {
    s.push(';');
    s.push_str(key);
    s.push('=');
    s.push_str(&pct_encode(val));
}

pub fn pct_encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for &b in s.as_bytes() {
        if is_unreserved(b) {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn is_unreserved(b: u8) -> bool {
    matches!(
        b,
        b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'/' | b':' | b'\\' | b'~'
    )
}

/// Write OSC 7880 to the real TTY fd (the pager redirects process stderr to `/dev/null`).
/// Event-loop code must use [`crate::render::draw::EscapeWriter`] instead — locking
/// stderr from that thread deadlocks the writer.
pub fn emit(req: &HostForkSplit) {
    if cfg!(test) {
        return;
    }
    let bytes = encode_osc(req);
    xai_grok_shell::util::with_locked_stderr(|stderr| {
        let _ = stderr.write_all(&bytes);
        let _ = stderr.flush();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encode_osc_contains_keys_and_bel() {
        let bytes = encode_osc(&HostForkSplit {
            resume: "sess-1".into(),
            cwd: PathBuf::from("/tmp/proj"),
            bin: PathBuf::from("/usr/bin/grok-fork"),
            prompt: Some("try the async approach".into()),
            title: None,
            brand_fork: true,
            new_session: false,
        });
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("\u{1b}]7880;fork=1;"));
        assert!(s.ends_with('\u{7}'));
        assert!(s.contains("resume=sess-1"));
        assert!(s.contains("bin=/usr/bin/grok-fork"));
        assert!(s.contains("brand=fork"));
        assert!(s.contains("prompt=try"));
        assert!(s.contains("%20"), "spaces in prompt are percent-encoded");
    }

    #[test]
    fn encode_osc_new_session_uses_session_key() {
        let bytes = encode_osc(&HostForkSplit {
            resume: "sess-new".into(),
            cwd: PathBuf::from("/tmp/proj"),
            bin: PathBuf::from("/usr/bin/grok-fork"),
            prompt: Some("own the review".into()),
            title: Some("review".into()),
            brand_fork: true,
            new_session: true,
        });
        let s = String::from_utf8(bytes).unwrap();
        assert!(s.starts_with("\u{1b}]7880;new=1;"));
        assert!(s.contains("session=sess-new"));
        assert!(!s.contains("resume="));
        assert!(!s.contains("fork=1"));
        assert!(s.contains("title=review"));
        assert!(s.ends_with('\u{7}'));
    }

    #[test]
    fn pct_roundtrip_spaces_and_plus() {
        let in_ = "a b+c";
        let enc = pct_encode(in_);
        assert_eq!(enc, "a%20b%2Bc");
    }

    #[test]
    fn should_host_split_auto_requires_env() {
        assert!(!should_host_split(false, ForkHostSplitMode::Auto, None));
        assert!(should_host_split(true, ForkHostSplitMode::Auto, None));
        assert!(!should_host_split(true, ForkHostSplitMode::Never, None));
        assert!(should_host_split(
            true,
            ForkHostSplitMode::Never,
            Some(true)
        ));
        assert!(!should_host_split(
            true,
            ForkHostSplitMode::Auto,
            Some(false)
        ));
        assert!(!should_host_split(
            false,
            ForkHostSplitMode::Auto,
            Some(true)
        ));
    }
}
