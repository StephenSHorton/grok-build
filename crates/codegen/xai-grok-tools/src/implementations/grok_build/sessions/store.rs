//! On-disk session bus: roster, exclusive roles, mailbox, live inject.
//!
//! Layout under `root` (default `$GROK_HOME/bus`):
//! ```text
//! roster.json
//! roles.json
//! bus.lock
//! live/<session-id>.sock      # unix domain socket (unix only)
//! mail/<session-id>/<message-id>.json
//! ```
//!
//! Windows live inject uses `\\.\pipe\grok-bus-<id>` (named pipes do not appear
//! under `live/`). `is_live` probes the pipe with `WaitNamedPipeW`.

use std::collections::BTreeMap;
use std::ffi::OsString;
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Write};
use std::path::{Path, PathBuf};

use fs2::FileExt;
use serde::{Deserialize, Serialize};

pub const MAILBOX_CAP: usize = 50;
const FILE_MODE: u32 = 0o600;
const ROSTER_VERSION: u32 = 1;
const ROLES_VERSION: u32 = 1;

#[derive(Debug, thiserror::Error)]
pub enum BusError {
    #[error("invalid role slug {0:?} (use lowercase letters, digits, hyphen, underscore)")]
    InvalidRole(String),
    #[error("role {role} is held by live session {session_id}")]
    RoleHeld { role: String, session_id: String },
    #[error("no session has claimed role {0}")]
    RoleNotFound(String),
    #[error("session {0} is not in the roster")]
    SessionNotFound(String),
    #[error("this conversation has not claimed role {0}")]
    RoleNotHeld(String),
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error(transparent)]
    Json(#[from] serde_json::Error),
}

pub type BusResult<T> = Result<T, BusError>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Talkable {
    Inject,
    Mailbox,
    Gone,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RosterEntry {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_summary: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
    pub last_active: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ListedSession {
    pub session_id: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub cwd: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub last_turn_summary: Option<String>,
    #[serde(default)]
    pub roles: Vec<String>,
    pub live: bool,
    pub talkable: Talkable,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PeerMessage {
    pub message_id: String,
    pub from_session: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub from_title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub role: Option<String>,
    pub content: String,
    pub created_at: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "snake_case")]
pub enum Delivery {
    Inject,
    Mailbox,
}

#[derive(Serialize, Deserialize)]
struct RosterFile {
    version: u32,
    sessions: BTreeMap<String, RosterEntry>,
}

#[derive(Serialize, Deserialize)]
struct RoleClaim {
    session_id: String,
    claimed_at: String,
}

#[derive(Serialize, Deserialize)]
struct RolesFile {
    version: u32,
    roles: BTreeMap<String, RoleClaim>,
}

pub struct SessionBus {
    root: PathBuf,
}

impl SessionBus {
    pub fn open(root: impl Into<PathBuf>) -> BusResult<Self> {
        let root = root.into();
        xai_grok_config::create_dir_all_owner_only(&root)?;
        xai_grok_config::create_dir_all_owner_only(&root.join("live"))?;
        xai_grok_config::create_dir_all_owner_only(&root.join("mail"))?;
        Ok(Self { root })
    }

    pub fn open_default() -> BusResult<Self> {
        Self::open(xai_grok_config::grok_home().join("bus"))
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn live_socket_path(&self, session_id: &str) -> BusResult<PathBuf> {
        Ok(self
            .root
            .join("live")
            .join(format!("{}.sock", sanitize_session_id(session_id)?)))
    }

    /// Windows named-pipe path (`\\.\pipe\grok-bus-<id>`). Unix callers do not use this.
    pub fn live_pipe_name(&self, session_id: &str) -> BusResult<OsString> {
        let _ = self;
        let id = sanitize_session_id(session_id)?;
        let mut name = OsString::from(r"\\.\pipe\");
        name.push(format!("grok-bus-{id}"));
        Ok(name)
    }

    fn live_inject_endpoint(&self, session_id: &str) -> BusResult<PathBuf> {
        #[cfg(unix)]
        {
            self.live_socket_path(session_id)
        }
        #[cfg(windows)]
        {
            Ok(PathBuf::from(self.live_pipe_name(session_id)?))
        }
        #[cfg(not(any(unix, windows)))]
        {
            self.live_socket_path(session_id)
        }
    }

    fn lock(&self) -> io::Result<File> {
        let path = self.root.join("bus.lock");
        let mut options = OpenOptions::new();
        options.create(true).read(true).write(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt as _;
            options.mode(FILE_MODE);
        }
        let file = options.open(path)?;
        file.lock_exclusive()?;
        Ok(file)
    }

    fn with_lock<T>(&self, f: impl FnOnce() -> BusResult<T>) -> BusResult<T> {
        let _guard = self.lock()?;
        f()
    }

    fn roster_path(&self) -> PathBuf {
        self.root.join("roster.json")
    }

    fn roles_path(&self) -> PathBuf {
        self.root.join("roles.json")
    }

    fn mail_dir(&self, session_id: &str) -> BusResult<PathBuf> {
        Ok(self
            .root
            .join("mail")
            .join(sanitize_session_id(session_id)?))
    }

    fn read_roster(&self) -> BusResult<RosterFile> {
        read_json(&self.roster_path(), || RosterFile {
            version: ROSTER_VERSION,
            sessions: BTreeMap::new(),
        })
    }

    fn write_roster(&self, file: &RosterFile) -> BusResult<()> {
        write_json(&self.roster_path(), file)
    }

    fn read_roles(&self) -> BusResult<RolesFile> {
        read_json(&self.roles_path(), || RolesFile {
            version: ROLES_VERSION,
            roles: BTreeMap::new(),
        })
    }

    fn write_roles(&self, file: &RolesFile) -> BusResult<()> {
        write_json(&self.roles_path(), file)
    }

    /// Upsert this session into the roster. Called on start and on claim.
    pub fn register(
        &self,
        session_id: &str,
        title: Option<String>,
        cwd: Option<String>,
        last_turn_summary: Option<String>,
        pid: Option<u32>,
    ) -> BusResult<()> {
        let id = sanitize_session_id(session_id)?.to_string();
        self.with_lock(|| {
            let mut roster = self.read_roster()?;
            let mut title = title.filter(|t| !t.trim().is_empty());
            if title.is_none() {
                title = roster
                    .sessions
                    .get(&id)
                    .and_then(|e| e.title.clone())
                    .filter(|t| !t.trim().is_empty())
                    .or_else(|| summary_title_on_disk(&self.root, &id));
            }
            roster.sessions.insert(
                id.clone(),
                RosterEntry {
                    session_id: id,
                    title,
                    cwd,
                    last_turn_summary,
                    pid,
                    last_active: now_rfc3339(),
                },
            );
            self.write_roster(&roster)
        })
    }

    /// Best title for a session: roster, then `summary.json` on disk. Never returns a raw session id.
    pub fn title_for_session(&self, session_id: &str) -> Option<String> {
        let from_roster = self
            .with_lock(|| {
                Ok(self
                    .read_roster()?
                    .sessions
                    .get(session_id)
                    .and_then(|e| e.title.clone()))
            })
            .ok()
            .flatten()
            .filter(|t| !t.trim().is_empty() && !looks_like_session_id(t));
        from_roster.or_else(|| summary_title_on_disk(&self.root, session_id))
    }

    pub fn unregister_live(&self, session_id: &str) -> BusResult<()> {
        let sock = self.live_socket_path(session_id)?;
        let _ = fs::remove_file(&sock);
        self.with_lock(|| {
            let mut roster = self.read_roster()?;
            if let Some(entry) = roster.sessions.get_mut(session_id) {
                entry.pid = None;
                entry.last_active = now_rfc3339();
            }
            self.write_roster(&roster)
        })
    }

    pub fn is_live(&self, session_id: &str) -> bool {
        #[cfg(unix)]
        {
            self.live_socket_path(session_id)
                .map(|p| p.exists())
                .unwrap_or(false)
        }
        #[cfg(windows)]
        {
            self.live_pipe_name(session_id)
                .ok()
                .is_some_and(|name| named_pipe_is_ready(&name))
        }
        #[cfg(not(any(unix, windows)))]
        {
            false
        }
    }

    pub fn prune_dead(&self) -> BusResult<()> {
        self.with_lock(|| self.prune_dead_locked())
    }

    fn prune_dead_locked(&self) -> BusResult<()> {
        let mut roster = self.read_roster()?;
        let mut dirty = false;
        for entry in roster.sessions.values_mut() {
            let live = self.is_live(&entry.session_id);
            let pid_dead = entry.pid.is_some_and(|pid| !pid_is_alive(pid));
            // Do not treat a missing listener as death while the pid is still
            // alive: Windows has no sock file, and unix bind happens after register.
            if pid_dead && !live {
                if let Ok(sock) = self.live_socket_path(&entry.session_id) {
                    let _ = fs::remove_file(sock);
                }
                entry.pid = None;
                dirty = true;
            }
        }
        if dirty {
            self.write_roster(&roster)?;
        }
        Ok(())
    }

    pub fn list(
        &self,
        role: Option<&str>,
        live_only: bool,
        query: Option<&str>,
    ) -> BusResult<Vec<ListedSession>> {
        self.with_lock(|| {
            self.prune_dead_locked()?;
            let roster = self.read_roster()?;
            let roles = self.read_roles()?;
            let q = query.map(|s| s.to_ascii_lowercase());
            let mut out = Vec::new();
            for entry in roster.sessions.values() {
                let held: Vec<String> = roles
                    .roles
                    .iter()
                    .filter(|(_, c)| c.session_id == entry.session_id)
                    .map(|(r, _)| r.clone())
                    .collect();
                if let Some(want) = role
                    && !held.iter().any(|r| r == want)
                {
                    continue;
                }
                let live = self.is_live(&entry.session_id);
                if live_only && !live {
                    continue;
                }
                if let Some(q) = &q {
                    let hay = format!(
                        "{} {} {} {}",
                        entry.session_id,
                        entry.title.as_deref().unwrap_or(""),
                        entry.cwd.as_deref().unwrap_or(""),
                        held.join(" ")
                    )
                    .to_ascii_lowercase();
                    if !hay.contains(q) {
                        continue;
                    }
                }
                out.push(ListedSession {
                    session_id: entry.session_id.clone(),
                    title: entry.title.clone(),
                    cwd: entry.cwd.clone(),
                    last_turn_summary: entry.last_turn_summary.clone(),
                    roles: held,
                    live,
                    talkable: if live {
                        Talkable::Inject
                    } else {
                        Talkable::Mailbox
                    },
                });
            }
            out.sort_by(|a, b| b.live.cmp(&a.live).then(a.session_id.cmp(&b.session_id)));
            Ok(out)
        })
    }

    pub fn claim(&self, session_id: &str, role: &str) -> BusResult<()> {
        let role = validate_role(role)?;
        let session_id = sanitize_session_id(session_id)?.to_string();
        self.with_lock(|| {
            self.prune_dead_locked()?;
            let mut roles = self.read_roles()?;
            if let Some(existing) = roles.roles.get(&role) {
                if existing.session_id != session_id {
                    if self.is_live(&existing.session_id) {
                        return Err(BusError::RoleHeld {
                            role,
                            session_id: existing.session_id.clone(),
                        });
                    }
                    // Dead holder: steal.
                }
            }
            roles.roles.insert(
                role,
                RoleClaim {
                    session_id,
                    claimed_at: now_rfc3339(),
                },
            );
            self.write_roles(&roles)
        })
    }

    pub fn release(&self, session_id: &str, role: &str) -> BusResult<()> {
        let role = validate_role(role)?;
        let session_id = sanitize_session_id(session_id)?.to_string();
        self.with_lock(|| {
            let mut roles = self.read_roles()?;
            match roles.roles.get(&role) {
                Some(c) if c.session_id == session_id => {
                    roles.roles.remove(&role);
                    self.write_roles(&roles)
                }
                Some(_) => Err(BusError::RoleNotHeld(role)),
                None => Err(BusError::RoleNotFound(role)),
            }
        })
    }

    /// Roster pid for a session, if the last register recorded one.
    pub fn session_pid(&self, session_id: &str) -> BusResult<Option<u32>> {
        self.with_lock(|| {
            Ok(self
                .read_roster()?
                .sessions
                .get(session_id)
                .and_then(|e| e.pid))
        })
    }

    pub fn resolve_target(&self, to: &str) -> BusResult<String> {
        self.with_lock(|| {
            self.prune_dead_locked()?;
            let roles = self.read_roles()?;
            if let Ok(role) = validate_role(to)
                && let Some(claim) = roles.roles.get(&role)
            {
                return Ok(claim.session_id.clone());
            }
            let roster = self.read_roster()?;
            if roster.sessions.contains_key(to) {
                return Ok(to.to_string());
            }
            if validate_role(to).is_ok() {
                Err(BusError::RoleNotFound(to.to_string()))
            } else {
                Err(BusError::SessionNotFound(to.to_string()))
            }
        })
    }

    pub fn enqueue_mail(&self, to_session: &str, msg: &PeerMessage) -> BusResult<()> {
        let dir = self.mail_dir(to_session)?;
        self.with_lock(|| {
            xai_grok_config::create_dir_all_owner_only(&dir)?;
            let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect();
            files.sort();
            while files.len() >= MAILBOX_CAP {
                if let Some(oldest) = files.first() {
                    let _ = fs::remove_file(oldest);
                }
                files.remove(0);
            }
            let path = dir.join(format!("{}.json", sanitize_message_id(&msg.message_id)?));
            write_json(&path, msg)
        })
    }

    pub fn take_mailbox(&self, session_id: &str) -> BusResult<Vec<PeerMessage>> {
        let dir = self.mail_dir(session_id)?;
        self.with_lock(|| {
            if !dir.exists() {
                return Ok(Vec::new());
            }
            let mut files: Vec<PathBuf> = fs::read_dir(&dir)?
                .filter_map(|e| e.ok().map(|e| e.path()))
                .filter(|p| p.extension().is_some_and(|e| e == "json"))
                .collect();
            files.sort();
            let mut out = Vec::new();
            for path in files {
                match fs::read_to_string(&path) {
                    Ok(raw) => {
                        if let Ok(msg) = serde_json::from_str::<PeerMessage>(&raw) {
                            out.push(msg);
                        }
                        let _ = fs::remove_file(&path);
                    }
                    Err(_) => {
                        let _ = fs::remove_file(&path);
                    }
                }
            }
            Ok(out)
        })
    }

    /// Try a live inject; on failure enqueue mailbox. Returns how it was delivered.
    pub fn send(&self, to_session: &str, msg: &PeerMessage) -> BusResult<Delivery> {
        if self.is_live(to_session) {
            match write_live_frame(&self.live_inject_endpoint(to_session)?, msg) {
                Ok(()) => return Ok(Delivery::Inject),
                Err(err) => {
                    tracing::debug!(
                        error = %err,
                        session = to_session,
                        "live inject failed; falling back to mailbox"
                    );
                    #[cfg(unix)]
                    {
                        if matches!(
                            err.kind(),
                            io::ErrorKind::NotFound | io::ErrorKind::ConnectionRefused
                        ) {
                            let _ = fs::remove_file(self.live_socket_path(to_session)?);
                        }
                    }
                }
            }
        }
        self.enqueue_mail(to_session, msg)?;
        Ok(Delivery::Mailbox)
    }
}

/// Label for a peer row: session title, else a role slug, else `"grok chat"`. Never a raw session uuid.
pub fn peer_display_label(to: &str, title: Option<&str>) -> String {
    if let Some(t) = title
        .map(str::trim)
        .filter(|s| !s.is_empty() && !looks_like_session_id(s))
    {
        return t.to_string();
    }
    if !looks_like_session_id(to) {
        return to.to_string();
    }
    "grok chat".to_string()
}

pub fn looks_like_session_id(s: &str) -> bool {
    s.len() >= 20 && s.bytes().filter(|b| *b == b'-').count() >= 3
}

fn summary_title_on_disk(bus_root: &Path, session_id: &str) -> Option<String> {
    let sessions = bus_root.parent()?.join("sessions");
    let entries = fs::read_dir(sessions).ok()?;
    for entry in entries {
        let candidate = entry.ok()?.path().join(session_id).join("summary.json");
        if !candidate.is_file() {
            continue;
        }
        let raw = fs::read_to_string(candidate).ok()?;
        let v: serde_json::Value = serde_json::from_str(&raw).ok()?;
        let title = v
            .get("generated_title")
            .or_else(|| v.get("custom_title"))
            .or_else(|| v.get("title"))
            .and_then(|x| x.as_str())
            .map(str::trim)
            .filter(|s| !s.is_empty() && !looks_like_session_id(s))?;
        return Some(title.to_string());
    }
    None
}

pub fn validate_role(role: &str) -> BusResult<String> {
    let ok = !role.is_empty()
        && role.len() <= 64
        && role.chars().next().is_some_and(|c| c.is_ascii_lowercase())
        && role
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_');
    if ok {
        Ok(role.to_string())
    } else {
        Err(BusError::InvalidRole(role.to_string()))
    }
}

fn sanitize_session_id(id: &str) -> BusResult<&str> {
    if id.is_empty()
        || id.len() > 128
        || !id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(BusError::SessionNotFound(id.to_string()));
    }
    Ok(id)
}

fn sanitize_message_id(id: &str) -> BusResult<&str> {
    sanitize_session_id(id)
}

fn now_rfc3339() -> String {
    chrono::Utc::now().to_rfc3339()
}

fn read_json<T: for<'de> Deserialize<'de>>(path: &Path, empty: impl FnOnce() -> T) -> BusResult<T> {
    if !path.exists() {
        return Ok(empty());
    }
    let raw = fs::read_to_string(path)?;
    if raw.trim().is_empty() {
        return Ok(empty());
    }
    Ok(serde_json::from_str(&raw)?)
}

fn write_json<T: Serialize>(path: &Path, value: &T) -> BusResult<()> {
    let contents = serde_json::to_string_pretty(value)? + "\n";
    xai_grok_config::fs_atomic::write_atomically(path, &contents, Some(FILE_MODE))?;
    Ok(())
}

pub(crate) fn pid_is_alive(pid: u32) -> bool {
    if pid == 0 {
        return false;
    }
    #[cfg(unix)]
    {
        nix::sys::signal::kill(nix::unistd::Pid::from_raw(pid as i32), None).is_ok()
    }
    #[cfg(windows)]
    {
        use windows::Win32::Foundation::{CloseHandle, WAIT_TIMEOUT};
        use windows::Win32::System::Threading::{
            OpenProcess, PROCESS_SYNCHRONIZE, WaitForSingleObject,
        };
        let Ok(handle) = (unsafe { OpenProcess(PROCESS_SYNCHRONIZE, false, pid) }) else {
            return false;
        };
        let wait_result = unsafe { WaitForSingleObject(handle, 0) };
        let _ = unsafe { CloseHandle(handle) };
        wait_result == WAIT_TIMEOUT
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = pid;
        false
    }
}

#[cfg(windows)]
fn named_pipe_is_ready(name: &std::ffi::OsStr) -> bool {
    use std::os::windows::ffi::OsStrExt;

    use windows::Win32::Foundation::{ERROR_FILE_NOT_FOUND, GetLastError};
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    use windows::core::PCWSTR;

    const PROBE_TIMEOUT_MS: u32 = 1;
    let wide: Vec<u16> = name.encode_wide().chain(std::iter::once(0)).collect();
    if unsafe { WaitNamedPipeW(PCWSTR(wide.as_ptr()), PROBE_TIMEOUT_MS) }.as_bool() {
        return true;
    }
    let err = unsafe { GetLastError() };
    err != ERROR_FILE_NOT_FOUND
}

pub fn write_live_frame(endpoint: &Path, msg: &PeerMessage) -> io::Result<()> {
    let bytes = serde_json::to_vec(msg).map_err(io::Error::other)?;
    let len = u32::try_from(bytes.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "peer message too large"))?;
    #[cfg(unix)]
    {
        let mut stream = std::os::unix::net::UnixStream::connect(endpoint)?;
        stream.write_all(&len.to_be_bytes())?;
        stream.write_all(&bytes)?;
        stream.flush()?;
        Ok(())
    }
    #[cfg(windows)]
    {
        write_windows_pipe_frame(endpoint, &len.to_be_bytes(), &bytes)
    }
    #[cfg(not(any(unix, windows)))]
    {
        let _ = (endpoint, bytes, len);
        Err(io::Error::new(
            io::ErrorKind::Unsupported,
            "live inject requires unix sockets or windows named pipes",
        ))
    }
}

#[cfg(windows)]
fn write_windows_pipe_frame(endpoint: &Path, len: &[u8], body: &[u8]) -> io::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use std::thread;
    use std::time::Duration;

    use windows::Win32::Foundation::ERROR_PIPE_BUSY;
    use windows::Win32::System::Pipes::WaitNamedPipeW;
    use windows::core::PCWSTR;

    const ATTEMPTS: usize = 40;
    const BUSY: i32 = ERROR_PIPE_BUSY.0 as i32;
    let wide: Vec<u16> = endpoint
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    for _ in 0..ATTEMPTS {
        unsafe {
            let _ = WaitNamedPipeW(PCWSTR(wide.as_ptr()), 50);
        }
        match OpenOptions::new().read(true).write(true).open(endpoint) {
            Ok(mut f) => {
                f.write_all(len)?;
                f.write_all(body)?;
                f.flush()?;
                return Ok(());
            }
            Err(err)
                if err.raw_os_error() == Some(BUSY)
                    || matches!(
                        err.kind(),
                        io::ErrorKind::NotFound | io::ErrorKind::WouldBlock
                    ) =>
            {
                thread::sleep(Duration::from_millis(25));
            }
            Err(err) => return Err(err),
        }
    }
    Err(io::Error::new(
        io::ErrorKind::TimedOut,
        "named pipe busy or missing",
    ))
}

pub fn read_live_frame(mut reader: impl Read) -> io::Result<PeerMessage> {
    let mut len_buf = [0u8; 4];
    reader.read_exact(&mut len_buf)?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 64 * 1024 {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "peer message too large",
        ));
    }
    let mut buf = vec![0u8; len];
    reader.read_exact(&mut buf)?;
    serde_json::from_slice(&buf).map_err(io::Error::other)
}

pub fn peer_message_to_prompt(msg: &PeerMessage) -> String {
    fn esc(s: &str) -> String {
        s.replace('&', "&amp;")
            .replace('<', "&lt;")
            .replace('>', "&gt;")
            .replace('"', "&quot;")
    }
    let mut tag = format!(
        "<channel source=\"session\" from_session=\"{}\" message_id=\"{}\"",
        esc(&msg.from_session),
        esc(&msg.message_id)
    );
    if let Some(title) = &msg.from_title {
        tag.push_str(&format!(" from_title=\"{}\"", esc(title)));
    }
    if let Some(role) = &msg.role {
        tag.push_str(&format!(" role=\"{}\"", esc(role)));
    }
    tag.push('>');
    if msg.content.is_empty() {
        tag.push_str("</channel>");
        return tag;
    }
    let body = msg
        .content
        .replace('&', "&amp;")
        .replace('<', "&lt;")
        .replace('>', "&gt;");
    format!("{tag}\n{body}\n</channel>")
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::thread;
    use std::time::Duration;

    fn bus() -> (tempfile::TempDir, SessionBus) {
        let dir = tempfile::tempdir().unwrap();
        let bus = SessionBus::open(dir.path()).unwrap();
        (dir, bus)
    }

    fn msg(from: &str, content: &str) -> PeerMessage {
        PeerMessage {
            message_id: uuid::Uuid::now_v7().to_string(),
            from_session: from.into(),
            from_title: Some("eng".into()),
            role: Some("pr-reviews".into()),
            content: content.into(),
            created_at: now_rfc3339(),
        }
    }

    #[test]
    fn exclusive_claim_blocks_second_live_holder() {
        let (_dir, bus) = bus();
        #[cfg(unix)]
        let _keep_live = {
            let sock = bus.live_socket_path("sess-a").unwrap();
            std::os::unix::net::UnixListener::bind(&sock).unwrap()
        };
        #[cfg(windows)]
        let _keep_live = {
            let name = bus.live_pipe_name("sess-a").unwrap();
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                tokio::net::windows::named_pipe::ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&name)
                    .unwrap()
            })
        };
        bus.register("sess-a", None, None, None, Some(std::process::id()))
            .unwrap();
        bus.claim("sess-a", "pr-reviews").unwrap();
        bus.register("sess-b", None, None, None, Some(std::process::id()))
            .unwrap();
        let err = bus.claim("sess-b", "pr-reviews").unwrap_err();
        match err {
            BusError::RoleHeld { role, session_id } => {
                assert_eq!(role, "pr-reviews");
                assert_eq!(session_id, "sess-a");
            }
            other => panic!("expected RoleHeld, got {other:?}"),
        }
    }

    #[test]
    fn pid_is_alive_reports_current_process() {
        assert!(pid_is_alive(std::process::id()));
        assert!(!pid_is_alive(0));
    }

    #[test]
    fn dead_holder_can_be_stolen() {
        let (_dir, bus) = bus();
        bus.register("dead", None, None, None, Some(1)).unwrap();
        bus.claim("dead", "pr-reviews").unwrap();
        bus.register("alive", None, None, None, Some(2)).unwrap();
        bus.claim("alive", "pr-reviews").unwrap();
        let listed = bus.list(Some("pr-reviews"), false, None).unwrap();
        assert_eq!(listed.len(), 1);
        assert_eq!(listed[0].session_id, "alive");
    }

    #[test]
    fn mailbox_round_trip_and_cap() {
        let (_dir, bus) = bus();
        for i in 0..(MAILBOX_CAP + 3) {
            let mut m = msg("from", &format!("n{i}"));
            m.message_id = format!("m{i:03}");
            bus.enqueue_mail("target", &m).unwrap();
        }
        let taken = bus.take_mailbox("target").unwrap();
        assert_eq!(taken.len(), MAILBOX_CAP);
        assert_eq!(taken[0].content, "n3");
        assert!(bus.take_mailbox("target").unwrap().is_empty());
    }

    #[test]
    fn send_to_missing_role_errors() {
        let (_dir, bus) = bus();
        let err = bus.resolve_target("pr-reviews").unwrap_err();
        assert!(matches!(err, BusError::RoleNotFound(_)));
    }

    #[cfg(unix)]
    #[test]
    fn live_send_injects_one_frame() {
        let (_dir, bus) = bus();
        let sock = bus.live_socket_path("sess-a").unwrap();
        let listener = std::os::unix::net::UnixListener::bind(&sock).unwrap();
        let handle = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            read_live_frame(stream).unwrap()
        });
        thread::sleep(Duration::from_millis(20));
        bus.register("sess-a", None, None, None, Some(std::process::id()))
            .unwrap();
        let outgoing = msg("sess-b", "please review");
        let delivery = bus.send("sess-a", &outgoing).unwrap();
        assert_eq!(delivery, Delivery::Inject);
        let got = handle.join().unwrap();
        assert_eq!(got.content, "please review");
        assert_eq!(got.from_session, "sess-b");
    }

    #[cfg(windows)]
    #[test]
    fn live_send_injects_one_frame() {
        use tokio::io::AsyncReadExt;

        let (_dir, bus) = bus();
        let name = bus.live_pipe_name("sess-a").unwrap();
        let handle = thread::spawn(move || {
            let rt = tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap();
            rt.block_on(async {
                let server = tokio::net::windows::named_pipe::ServerOptions::new()
                    .first_pipe_instance(true)
                    .create(&name)
                    .unwrap();
                server.connect().await.unwrap();
                let mut len_buf = [0u8; 4];
                let mut server = server;
                server.read_exact(&mut len_buf).await.unwrap();
                let len = u32::from_be_bytes(len_buf) as usize;
                let mut buf = vec![0u8; len];
                server.read_exact(&mut buf).await.unwrap();
                serde_json::from_slice::<PeerMessage>(&buf).unwrap()
            })
        });
        thread::sleep(Duration::from_millis(50));
        bus.register("sess-a", None, None, None, Some(std::process::id()))
            .unwrap();
        let outgoing = msg("sess-b", "please review");
        let delivery = bus.send("sess-a", &outgoing).unwrap();
        assert_eq!(delivery, Delivery::Inject);
        let got = handle.join().unwrap();
        assert_eq!(got.content, "please review");
        assert_eq!(got.from_session, "sess-b");
    }

    #[test]
    fn dormant_send_writes_mail() {
        let (_dir, bus) = bus();
        bus.register("sess-a", Some("duty".into()), None, None, None)
            .unwrap();
        let outgoing = msg("sess-b", "please review");
        let delivery = bus.send("sess-a", &outgoing).unwrap();
        assert_eq!(delivery, Delivery::Mailbox);
        let mail = bus.take_mailbox("sess-a").unwrap();
        assert_eq!(mail.len(), 1);
        assert_eq!(mail[0].content, "please review");
    }

    #[test]
    fn peer_display_label_prefers_title_over_session_id() {
        assert_eq!(
            peer_display_label("01a09207-8741-7d22-b7ae-32cb89fc3cb9", Some("smoke")),
            "smoke"
        );
        assert_eq!(peer_display_label("pr-reviews", None), "pr-reviews");
        assert_eq!(
            peer_display_label("01a09207-8741-7d22-b7ae-32cb89fc3cb9", None),
            "grok chat"
        );
    }

    #[test]
    fn invalid_role_rejected() {
        assert!(validate_role("PR").is_err());
        assert!(validate_role("pr reviews").is_err());
        assert!(validate_role("pr-reviews").is_ok());
    }

    #[test]
    fn peer_prompt_is_channel_xml() {
        let m = msg("abc", "hello <x>");
        let p = peer_message_to_prompt(&m);
        assert!(p.starts_with("<channel source=\"session\""));
        assert!(p.contains("from_session=\"abc\""));
        assert!(p.contains("hello &lt;x&gt;"));
        assert!(p.ends_with("</channel>"));
    }
}
