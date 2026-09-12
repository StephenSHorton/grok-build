//! Live socket + mailbox drain so sibling sessions can inject turns.

use tokio::io::AsyncReadExt;
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use xai_grok_tools::implementations::grok_build::sessions::{
    PeerMessage, SessionBus, peer_message_to_prompt,
};

use super::commands::{NotificationPriority, NotificationSource, SessionCommand};

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

    let sock = match bus.live_socket_path(&session_id) {
        Ok(p) => p,
        Err(err) => {
            tracing::warn!(error = %err, "session bus: socket path");
            return None;
        }
    };
    let _ = std::fs::remove_file(&sock);
    #[cfg(unix)]
    let listener = match tokio::net::UnixListener::bind(&sock) {
        Ok(l) => l,
        Err(err) => {
            tracing::warn!(error = %err, path = %sock.display(), "session bus: bind failed");
            return None;
        }
    };
    #[cfg(not(unix))]
    {
        let _ = (sock, cmd_tx);
        drain_mailbox_into(&bus, &session_id, &cmd_tx);
        return Some(SessionBusGuard {
            session_id,
            cancel: CancellationToken::new(),
        });
    }

    drain_mailbox_into(&bus, &session_id, &cmd_tx);

    let cancel = CancellationToken::new();
    let cancel_task = cancel.clone();
    let sid = session_id.clone();
    tokio::task::spawn_local(async move {
        loop {
            tokio::select! {
                _ = cancel_task.cancelled() => break,
                accepted = listener.accept() => {
                    match accepted {
                        Ok((stream, _)) => {
                            if let Err(err) = ingest_stream(stream, &cmd_tx).await {
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

    Some(SessionBusGuard { session_id, cancel })
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

#[cfg(unix)]
async fn ingest_stream(
    mut stream: tokio::net::UnixStream,
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
