# Session events

Grok-owned, **app-agnostic** inbound queue under `~/.grok/events/`.

Producers write JSON files. Each session **batch-drains** pending events at
turn start (soft inject as a `<system-reminder>`), then **deletes** them.
This is a doorbell work queue — not chat history and not pub/sub.

## Layout

```
~/.grok/events/
  by-session/<session-id>/
    evt_<uuid>.json
  by-name/
    pm          # plain text: session id
```

## Event envelope

```json
{
  "v": 1,
  "id": "evt_…",
  "ts": "2026-08-07T18:00:00Z",
  "source": "suzuri.workspace",
  "kind": "message",
  "title": "optional",
  "body": "…",
  "data": {},
  "target": { "session_id": "…", "name": null, "tag": null }
}
```

`source` / `kind` are free strings. Core never special-cases `suzuri`.

## Lifecycle

| Moment | Behavior |
|--------|----------|
| Enqueue | Write file; enforce cap (100) + TTL (7d) |
| Turn start | Drain all pending → one system-reminder → delete files |
| Mid-turn | New events wait for next turn |
| `/exit` (keep history) | Queue kept for resume |
| Permanent `/delete` | Queue dir deleted |
| Orphan session id | `gc_orphans` can drop |

## Tools

| Tool | Purpose |
|------|---------|
| `session_events_list` | Peek pending (no delete) |
| `session_events_drain` | Manual batch drain + clear |
| `session_events_enqueue` | Enqueue for a session_id or registered name |

## Producer (any language)

Write a JSON file under `~/.grok/events/by-session/<session-id>/`:

```bash
SID=019f….   # target session id
ID="evt_$(uuidgen | tr -d - | tr 'A-Z' 'a-z')"
mkdir -p "$HOME/.grok/events/by-session/$SID"
cat > "$HOME/.grok/events/by-session/$SID/${ID}.json" <<EOF
{
  "v": 1,
  "id": "$ID",
  "ts": "$(date -u +%Y-%m-%dT%H:%M:%SZ)",
  "source": "script.notify",
  "kind": "message",
  "body": "hello from outside",
  "target": { "session_id": "$SID" }
}
EOF
```

Next user turn in that session injects and clears the queue.

## Crate

`xai-grok-session-events` — pure filesystem API for enqueue / list / drain / GC.

## Not in v0

- Hard inject (auto-start a turn with no user message)
- Dashboard badge
- Public webhooks
- Cross-machine sync
