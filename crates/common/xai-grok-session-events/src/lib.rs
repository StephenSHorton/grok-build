//! Session events: a **Grok-owned**, app-agnostic work queue under `~/.grok/events/`.
//!
//! Producers (suzuri, CI, scripts, agents) write JSON event files. A session
//! **batch-drains** pending events at turn start (or via tools), then **deletes**
//! them. This is a doorbell queue, not a durable chat log and not pub/sub.
//!
//! Layout:
//! ```text
//! ~/.grok/events/
//!   by-session/<session-id>/*.json
//!   by-name/<alias> -> ../by-session/<id>   (optional symlink or pointer file)
//! ```

mod format;
mod store;

pub use format::{
    format_events_reminder, Event, EventTarget, DEFAULT_MAX_PENDING, DEFAULT_TTL_DAYS,
};
pub use store::{
    delete_session_events, drain_session, enqueue, events_root, gc_orphans, list_pending,
    pending_count, register_name, resolve_target, EnqueueOpts, SessionEventsError, Store,
};
