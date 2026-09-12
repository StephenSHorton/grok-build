//! Live inject + mailbox drain so sibling sessions can message each other.
//!
//! Unix: unix-domain socket at `live/<id>.sock`.
//! Windows: named pipe `\\.\pipe\grok-bus-<id>` (same pattern as leader IPC).
//! Both platforms also poll the mailbox so a failed inject still lands without
//! restarting the target.

use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio::time::{Duration, MissedTickBehavior};
use tokio_util::sync::CancellationToken;
use xai_grok_tools::implementations::grok_build::sessions::{
    PeerMessage, SessionBus, peer_message_to_prompt,
};

use super::commands::{NotificationPriority, NotificationSource, SessionCommand};

const MAILBOX_POLL: Duration = Duration::from_millis(250);

pub(crate) struct SessionBusGuard {
    session_id: String,
    cancel: CancellationToken,
}

impl Drop for SessionBusGuard {
    fn drop(&mut self) {
        self.cancel.cancel();
        if let Ok(bus) = SessionBus::open_default() {
            let _ = bus.unregister_live(&self.session_id);
        }
    }
}

pub(crate) fn spawn_session_bus(
    session_id: String,
    title: Option<String>,
    cwd: Option<String>,
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
) -> Option<SessionBusGuard> {
    let bus = match SessionBus::open_default() {
        Ok(b) => b,
        Err(err) => {
            tracing::warn!(error = %err, "session bus: failed to open");
            return None;
        }
    };
    let title = title.or_else(|| bus.title_for_session(&session_id));
    if let Err(err) = bus.register(&session_id, title, cwd, None, Some(std::process::id())) {
        tracing::warn!(error = %err, "session bus: register failed");
        return None;
    }

    drain_mailbox_into(&bus, &session_id, &cmd_tx);

    let cancel = CancellationToken::new();
    spawn_mailbox_poll(session_id.clone(), cmd_tx.clone(), cancel.clone());

    #[cfg(unix)]
    {
        if let Err(err) = spawn_unix_listener(&bus, &session_id, cmd_tx, cancel.clone()) {
            tracing::warn!(error = %err, "session bus: unix listener failed; mailbox poll still runs");
        }
    }

    #[cfg(windows)]
    {
        if let Err(err) = spawn_windows_listener(&bus, &session_id, cmd_tx, cancel.clone()) {
            tracing::warn!(error = %err, "session bus: named pipe failed; mailbox poll still runs");
        }
    }

    Some(SessionBusGuard { session_id, cancel })
}

fn spawn_mailbox_poll(
    session_id: String,
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    cancel: CancellationToken,
) {
    tokio::task::spawn_local(async move {
        let mut tick = tokio::time::interval(MAILBOX_POLL);
        tick.set_missed_tick_behavior(MissedTickBehavior::Skip);
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                _ = tick.tick() => {
                    if let Ok(bus) = SessionBus::open_default() {
                        drain_mailbox_into(&bus, &session_id, &cmd_tx);
                    }
                }
            }
        }
    });
}

#[cfg(unix)]
fn spawn_unix_listener(
    bus: &SessionBus,
    session_id: &str,
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    cancel: CancellationToken,
) -> std::io::Result<()> {
    let sock = bus
        .live_socket_path(session_id)
        .map_err(std::io::Error::other)?;
    let _ = std::fs::remove_file(&sock);
    let listener = tokio::net::UnixListener::bind(&sock)?;
    let sid = session_id.to_string();
    tokio::task::spawn_local(async move {
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            if let Err(err) = ingest_reader(stream, &cmd_tx).await {
                                tracing::debug!(error = %err, session = %sid, "session bus: ingest failed");
                            }
                        }
                        Err(err) => {
                            tracing::debug!(error = %err, "session bus: accept failed");
                            break;
                        }
                    }
                }
            }
        }
        let _ = std::fs::remove_file(&sock);
    });
    Ok(())
}

#[cfg(windows)]
fn spawn_windows_listener(
    bus: &SessionBus,
    session_id: &str,
    cmd_tx: mpsc::UnboundedSender<SessionCommand>,
    cancel: CancellationToken,
) -> std::io::Result<()> {
    use tokio::net::windows::named_pipe::ServerOptions;

    let pipe_name = bus.live_pipe_name(session_id).map_err(std::io::Error::other)?;
    let first = ServerOptions::new()
        .first_pipe_instance(true)
        .create(&pipe_name)?;
    let sid = session_id.to_string();
    tokio::task::spawn_local(async move {
        let mut server = first;
        loop {
            tokio::select! {
                _ = cancel.cancelled() => break,
                connected = server.connect() => {
                    match connected {
                        Ok(()) => {
                            let next = match ServerOptions::new().create(&pipe_name) {
                                Ok(s) => s,
                                Err(err) => {
                                    tracing::warn!(error = %err, "session bus: next pipe instance failed");
                                    let _ = ingest_reader(server, &cmd_tx).await;
                                    break;
                                }
                            };
                            let connected_server = std::mem::replace(&mut server, next);
                            let tx = cmd_tx.clone();
                            let sid = sid.clone();
                            tokio::task::spawn_local(async move {
                                if let Err(err) = ingest_reader(connected_server, &tx).await {
                                    tracing::debug!(error = %err, session = %sid, "session bus: ingest failed");
                                }
                            });
                        }
                        Err(err) => {
                            tracing::debug!(error = %err, "session bus: pipe connect failed");
                            break;
                        }
                    }
                }
            }
        }
    });
    Ok(())
}

fn drain_mailbox_into(
    bus: &SessionBus,
    session_id: &str,
    cmd_tx: &mpsc::UnboundedSender<SessionCommand>,
) {
    match bus.take_mailbox(session_id) {
        Ok(msgs) => {
            for msg in msgs {
                if cmd_tx.send(inject_cmd(msg)).is_err() {
                    break;
                }
            }
        }
        Err(err) => tracing::debug!(error = %err, "session bus: mailbox drain failed"),
    }
}

fn inject_cmd(msg: PeerMessage) -> SessionCommand {
    let prompt_id = format!("peer-session-{}", msg.message_id);
    SessionCommand::InjectNotification {
        prompt_id,
        prompt_blocks: vec![peer_content_block(&msg)],
        priority: NotificationPriority::Next,
        source: NotificationSource::Session {
            from_session: msg.from_session.clone(),
            from_title: msg.from_title.clone(),
            message_id: msg.message_id.clone(),
        },
    }
}

fn peer_content_block(msg: &PeerMessage) -> agent_client_protocol::ContentBlock {
    let mut meta = serde_json::Map::new();
    meta.insert("displayText".into(), serde_json::json!(msg.content.clone()));
    meta.insert("displayAsPeer".into(), serde_json::json!(true));
    if let Some(title) = &msg.from_title {
        meta.insert("peerFromTitle".into(), serde_json::json!(title));
    }
    meta.insert(
        "peerFromSession".into(),
        serde_json::json!(msg.from_session),
    );
    agent_client_protocol::ContentBlock::Text(
        agent_client_protocol::TextContent::new(peer_message_to_prompt(msg)).meta(Some(meta)),
    )
}

async fn ingest_reader<R: tokio::io::AsyncRead + Unpin>(
    mut stream: R,
    cmd_tx: &mpsc::UnboundedSender<SessionCommand>,
) -> std::io::Result<()> {
    let mut len_buf = [0u8; 4];
    stream.read_exact(&mut len_buf).await?;
    let len = u32::from_be_bytes(len_buf) as usize;
    if len > 64 * 1024 {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidData,
            "peer message too large",
        ));
    }
    let mut buf = vec![0u8; len];
    stream.read_exact(&mut buf).await?;
    let msg: PeerMessage = serde_json::from_slice(&buf).map_err(std::io::Error::other)?;
    cmd_tx.send(inject_cmd(msg)).map_err(|_| {
        std::io::Error::new(
            std::io::ErrorKind::BrokenPipe,
            "session command channel closed",
        )
    })?;
    Ok(())
}
