//! Filesystem store for session events.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use chrono::{TimeZone, Utc};

use crate::format::{Event, DEFAULT_MAX_PENDING, DEFAULT_TTL_DAYS};

#[derive(Debug, thiserror::Error)]
pub enum SessionEventsError {
    #[error("io: {0}")]
    Io(#[from] io::Error),
    #[error("json: {0}")]
    Json(#[from] serde_json::Error),
    #[error("target required (session_id or registered name)")]
    TargetRequired,
    #[error("unknown name alias: {0}")]
    UnknownName(String),
    #[error("invalid session id")]
    InvalidSessionId,
}

/// Options for enqueue.
#[derive(Debug, Clone)]
pub struct EnqueueOpts {
    pub max_pending: usize,
    pub ttl_days: u64,
}

impl Default for EnqueueOpts {
    fn default() -> Self {
        Self {
            max_pending: DEFAULT_MAX_PENDING,
            ttl_days: DEFAULT_TTL_DAYS,
        }
    }
}

/// Root: `$GROK_HOME/events` or `~/.grok/events`.
pub fn events_root() -> PathBuf {
    xai_grok_config::grok_home().join("events")
}

/// Store rooted at an explicit directory (tests use temp dirs).
#[derive(Debug, Clone)]
pub struct Store {
    root: PathBuf,
}

impl Default for Store {
    fn default() -> Self {
        Self {
            root: events_root(),
        }
    }
}

impl Store {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn session_dir(&self, session_id: &str) -> Result<PathBuf, SessionEventsError> {
        let id = sanitize_session_id(session_id)?;
        Ok(self.root.join("by-session").join(id))
    }

    /// Enqueue an event for a session. Applies TTL GC + cap for that session.
    pub fn enqueue(
        &self,
        session_id: &str,
        mut event: Event,
        opts: &EnqueueOpts,
    ) -> Result<PathBuf, SessionEventsError> {
        let dir = self.session_dir(session_id)?;
        fs::create_dir_all(&dir)?;
        event.target.session_id = Some(session_id.to_string());
        // Drop expired first.
        let _ = self.gc_session_ttl(session_id, opts.ttl_days);
        let path = dir.join(format!("{}.json", event.id));
        let tmp = dir.join(format!("{}.json.tmp", event.id));
        let raw = serde_json::to_vec_pretty(&event)?;
        fs::write(&tmp, raw)?;
        fs::rename(&tmp, &path)?;
        self.enforce_cap(session_id, opts.max_pending)?;
        Ok(path)
    }

    /// List pending events (oldest first). Does not delete.
    pub fn list_pending(&self, session_id: &str) -> Result<Vec<Event>, SessionEventsError> {
        let dir = self.session_dir(session_id)?;
        if !dir.is_dir() {
            return Ok(vec![]);
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.extension().and_then(|x| x.to_str()) == Some("json")
                    && !p
                        .file_name()
                        .and_then(|n| n.to_str())
                        .is_some_and(|n| n.ends_with(".tmp"))
            })
            .collect();
        paths.sort();
        let mut out = Vec::with_capacity(paths.len());
        for p in paths {
            match fs::read_to_string(&p) {
                Ok(s) => match serde_json::from_str::<Event>(&s) {
                    Ok(ev) => out.push(ev),
                    Err(_) => {
                        // Corrupt file — leave for GC/manual; skip.
                    }
                },
                Err(_) => {}
            }
        }
        Ok(out)
    }

    pub fn pending_count(&self, session_id: &str) -> Result<usize, SessionEventsError> {
        Ok(self.list_pending(session_id)?.len())
    }

    /// Batch drain: load all pending, delete files, return events (oldest first).
    pub fn drain(&self, session_id: &str) -> Result<Vec<Event>, SessionEventsError> {
        let dir = self.session_dir(session_id)?;
        if !dir.is_dir() {
            return Ok(vec![]);
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
            .collect();
        paths.sort();
        let mut events = Vec::with_capacity(paths.len());
        for p in &paths {
            if let Ok(s) = fs::read_to_string(p)
                && let Ok(ev) = serde_json::from_str::<Event>(&s)
            {
                events.push(ev);
            }
            let _ = fs::remove_file(p);
        }
        // Remove empty session dir.
        let _ = fs::remove_dir(&dir);
        Ok(events)
    }

    /// Delete the entire per-session event setup (session permanently gone).
    pub fn delete_session(&self, session_id: &str) -> Result<(), SessionEventsError> {
        let dir = self.session_dir(session_id)?;
        if dir.is_dir() {
            fs::remove_dir_all(&dir)?;
        }
        // Drop name aliases pointing at this id.
        let names = self.root.join("by-name");
        if names.is_dir() {
            for ent in fs::read_dir(&names)? {
                let ent = ent?;
                let p = ent.path();
                if let Ok(target) = fs::read_to_string(&p) {
                    if target.trim() == session_id {
                        let _ = fs::remove_file(&p);
                    }
                }
            }
        }
        Ok(())
    }

    /// Register a short name → session_id mapping (plain text file).
    pub fn register_name(&self, name: &str, session_id: &str) -> Result<(), SessionEventsError> {
        let name = sanitize_name(name)?;
        let _ = sanitize_session_id(session_id)?;
        let dir = self.root.join("by-name");
        fs::create_dir_all(&dir)?;
        let path = dir.join(&name);
        fs::write(path, session_id.trim())?;
        Ok(())
    }

    pub fn resolve_name(&self, name: &str) -> Result<String, SessionEventsError> {
        let name = sanitize_name(name)?;
        let path = self.root.join("by-name").join(&name);
        let s = fs::read_to_string(&path).map_err(|_| SessionEventsError::UnknownName(name))?;
        let id = s.trim().to_string();
        if id.is_empty() {
            return Err(SessionEventsError::UnknownName(s));
        }
        Ok(id)
    }

    /// Resolve target to a session id.
    pub fn resolve_target(
        &self,
        session_id: Option<&str>,
        name: Option<&str>,
    ) -> Result<String, SessionEventsError> {
        if let Some(id) = session_id.map(str::trim).filter(|s| !s.is_empty()) {
            return Ok(id.to_string());
        }
        if let Some(n) = name.map(str::trim).filter(|s| !s.is_empty()) {
            return self.resolve_name(n);
        }
        Err(SessionEventsError::TargetRequired)
    }

    fn enforce_cap(&self, session_id: &str, max: usize) -> Result<(), SessionEventsError> {
        if max == 0 {
            return Ok(());
        }
        let dir = self.session_dir(session_id)?;
        if !dir.is_dir() {
            return Ok(());
        }
        let mut paths: Vec<PathBuf> = fs::read_dir(&dir)?
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("json"))
            .collect();
        paths.sort();
        if paths.len() <= max {
            return Ok(());
        }
        let drop_n = paths.len() - max;
        for p in paths.into_iter().take(drop_n) {
            let _ = fs::remove_file(p);
        }
        Ok(())
    }

    fn gc_session_ttl(&self, session_id: &str, ttl_days: u64) -> Result<usize, SessionEventsError> {
        let dir = self.session_dir(session_id)?;
        if !dir.is_dir() {
            return Ok(0);
        }
        let cutoff = Utc::now() - chrono::Duration::days(ttl_days as i64);
        let mut removed = 0;
        for ent in fs::read_dir(&dir)? {
            let ent = ent?;
            let p = ent.path();
            if p.extension().and_then(|x| x.to_str()) != Some("json") {
                continue;
            }
            let meta = ent.metadata()?;
            let expired = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .and_then(|d| Utc.timestamp_opt(d.as_secs() as i64, 0).single())
                .is_some_and(|t| t < cutoff);
            if expired {
                let _ = fs::remove_file(&p);
                removed += 1;
            }
        }
        Ok(removed)
    }

    /// Remove event dirs for session ids not in `live_ids`. Also TTL-clean remaining.
    pub fn gc_orphans(
        &self,
        live_ids: &std::collections::HashSet<String>,
        ttl_days: u64,
    ) -> Result<usize, SessionEventsError> {
        let base = self.root.join("by-session");
        if !base.is_dir() {
            return Ok(0);
        }
        let mut removed = 0;
        for ent in fs::read_dir(&base)? {
            let ent = ent?;
            let name = ent.file_name().to_string_lossy().to_string();
            if !live_ids.contains(&name) {
                let p = ent.path();
                if p.is_dir() {
                    fs::remove_dir_all(&p)?;
                    removed += 1;
                }
            } else {
                removed += self.gc_session_ttl(&name, ttl_days)?;
            }
        }
        Ok(removed)
    }
}

// --- free functions on Default store ---

pub fn enqueue(
    session_id: &str,
    event: Event,
    opts: &EnqueueOpts,
) -> Result<PathBuf, SessionEventsError> {
    Store::default().enqueue(session_id, event, opts)
}

pub fn list_pending(session_id: &str) -> Result<Vec<Event>, SessionEventsError> {
    Store::default().list_pending(session_id)
}

pub fn pending_count(session_id: &str) -> Result<usize, SessionEventsError> {
    Store::default().pending_count(session_id)
}

pub fn drain_session(session_id: &str) -> Result<Vec<Event>, SessionEventsError> {
    Store::default().drain(session_id)
}

pub fn delete_session_events(session_id: &str) -> Result<(), SessionEventsError> {
    Store::default().delete_session(session_id)
}

pub fn register_name(name: &str, session_id: &str) -> Result<(), SessionEventsError> {
    Store::default().register_name(name, session_id)
}

pub fn resolve_target(
    session_id: Option<&str>,
    name: Option<&str>,
) -> Result<String, SessionEventsError> {
    Store::default().resolve_target(session_id, name)
}

pub fn gc_orphans(
    live_ids: &std::collections::HashSet<String>,
    ttl_days: u64,
) -> Result<usize, SessionEventsError> {
    Store::default().gc_orphans(live_ids, ttl_days)
}

fn sanitize_session_id(id: &str) -> Result<&str, SessionEventsError> {
    let id = id.trim();
    if id.is_empty()
        || id.contains('/')
        || id.contains('\\')
        || id.contains("..")
        || id.contains('\0')
    {
        return Err(SessionEventsError::InvalidSessionId);
    }
    Ok(id)
}

fn sanitize_name(name: &str) -> Result<String, SessionEventsError> {
    let name = name.trim().to_lowercase();
    if name.is_empty() || name.contains('/') || name.contains('\\') || name.contains("..") {
        return Err(SessionEventsError::InvalidSessionId);
    }
    let ok = name
        .chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_');
    if !ok {
        return Err(SessionEventsError::InvalidSessionId);
    }
    Ok(name)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::format_events_reminder;
    use std::collections::HashSet;

    #[test]
    fn enqueue_list_drain_clears() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let opts = EnqueueOpts::default();
        let e = Event::new("test.source", "message", "hello");
        store.enqueue("sess-1", e, &opts).unwrap();
        assert_eq!(store.pending_count("sess-1").unwrap(), 1);
        let listed = store.list_pending("sess-1").unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].body, "hello");
        let drained = store.drain("sess-1").unwrap();
        assert_eq!(drained.len(), 1);
        assert_eq!(store.pending_count("sess-1").unwrap(), 0);
        assert!(format_events_reminder(&drained).contains("hello"));
    }

    #[test]
    fn cap_drops_oldest() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let opts = EnqueueOpts {
            max_pending: 2,
            ttl_days: 7,
        };
        for i in 0..5 {
            let e = Event::new("s", "k", format!("b{i}"));
            // slight uniqueness in id via new uuid each time
            store.enqueue("s", e, &opts).unwrap();
        }
        let list = store.list_pending("s").unwrap();
        assert_eq!(list.len(), 2);
        assert!(list[0].body == "b3" || list[0].body == "b4" || list[1].body == "b4");
    }

    #[test]
    fn name_alias_and_delete() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store.register_name("pm", "sess-abc").unwrap();
        assert_eq!(store.resolve_name("pm").unwrap(), "sess-abc");
        store
            .enqueue(
                "sess-abc",
                Event::new("s", "k", "x"),
                &EnqueueOpts::default(),
            )
            .unwrap();
        store.delete_session("sess-abc").unwrap();
        assert_eq!(store.pending_count("sess-abc").unwrap(), 0);
        assert!(store.resolve_name("pm").is_err());
    }

    #[test]
    fn gc_orphans() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        store
            .enqueue("live", Event::new("s", "k", "a"), &EnqueueOpts::default())
            .unwrap();
        store
            .enqueue("dead", Event::new("s", "k", "b"), &EnqueueOpts::default())
            .unwrap();
        let mut live = HashSet::new();
        live.insert("live".into());
        store.gc_orphans(&live, 7).unwrap();
        assert_eq!(store.pending_count("live").unwrap(), 1);
        assert_eq!(store.pending_count("dead").unwrap(), 0);
    }

    #[test]
    fn batch_drain_multiple() {
        let dir = tempfile::tempdir().unwrap();
        let store = Store::new(dir.path());
        let opts = EnqueueOpts::default();
        store
            .enqueue("s", Event::new("a", "m", "one"), &opts)
            .unwrap();
        store
            .enqueue("s", Event::new("b", "m", "two"), &opts)
            .unwrap();
        let d = store.drain("s").unwrap();
        assert_eq!(d.len(), 2);
        assert!(store.list_pending("s").unwrap().is_empty());
    }
}
