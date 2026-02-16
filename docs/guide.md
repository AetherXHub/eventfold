# eventfold Concepts Guide

This guide covers how eventfold works, best practices for writing reducers, and practical advice for production use.

## 1. How It Works

Every piece of application state in eventfold is computed by folding events through a reducer function:

```
state = events.fold(initial_state, reducer)
```

### Lifecycle of an Event

```
  append()         flush            refresh()
     |               |                 |
     v               v                 v
 [Event] -----> [ app.jsonl ] -----> [ Reducer ] -----> [ State ]
                                         |
                                         v
                                 [ snapshot.json ]
```

1. **Append**: The event is serialized as a JSON line and appended to `app.jsonl`. The file is flushed and synced to disk.
2. **Persist**: The event is now durable. Even if the process crashes, the event survives.
3. **Refresh**: A view reads new events from `app.jsonl`, folds them through its reducer, and saves a snapshot of the resulting state.

### Data Flow

```
                    +-----------------+
                    |   Your Code     |
                    |                 |
                    |  append(event)  |
                    |  refresh_all()  |
                    |  view("todos")  |
                    +--------+--------+
                             |
                    +--------v--------+
                    |    EventLog     |
                    |                 |
                    |  app.jsonl      |  <-- active log (plain JSONL)
                    |  archive.zst   |  <-- rotated history (zstd)
                    |  views/         |  <-- snapshot cache
                    +-----------------+
```

### Directory Layout

```
data/
  app.jsonl                    # active event log (append-only JSONL)
  archive.jsonl.zst            # compressed event history (zstd frames)
  views/
    todos.snapshot.json         # cached state + offset + hash
    stats.snapshot.json
```

## 2. Writing Reducers

A reducer is a pure function with the signature:

```rust
fn my_reducer(state: MyState, event: &Event) -> MyState
```

### Best Practices

**Always handle unknown event types with a wildcard arm.** This is critical for forward compatibility — new event types should not break existing reducers.

```rust
fn reducer(mut state: State, event: &Event) -> State {
    match event.event_type.as_str() {
        "user_created" => { /* handle */ }
        "user_updated" => { /* handle */ }
        _ => {} // ignore unknown types
    }
    state
}
```

**Keep reducers pure.** No I/O, no network calls, no random values, no timestamps. The reducer should produce the same output given the same input, every time. This is what makes views rebuildable.

**Use `event.data` defensively.** Events are schemaless JSON. Use `as_str()`, `as_u64()`, etc. with `unwrap_or()` defaults rather than panicking on unexpected shapes.

```rust
let name = event.data["name"].as_str().unwrap_or("unknown");
let count = event.data["count"].as_u64().unwrap_or(0);
```

**Derive `Default` for your state.** Every view starts from `S::default()` — make sure the default is a valid empty state.

### Patterns

**Counter:**
```rust
fn count(state: u64, _event: &Event) -> u64 {
    state + 1
}
```

**Accumulator with filtering:**
```rust
fn error_log(mut state: Vec<String>, event: &Event) -> Vec<String> {
    if event.event_type == "error" {
        if let Some(msg) = event.data["message"].as_str() {
            state.push(msg.to_string());
        }
    }
    state
}
```

**Entity collection (CRUD):**
```rust
fn users(mut state: UserState, event: &Event) -> UserState {
    match event.event_type.as_str() {
        "user_created" => { /* insert */ }
        "user_updated" => { /* update in place */ }
        "user_deleted" => { /* remove */ }
        _ => {}
    }
    state
}
```

## 3. Multiple Views

A single event log can have any number of views. Each view has its own reducer, its own state type, and its own snapshot on disk. They all read from the same events.

```rust
let mut log = EventLog::builder("./data")
    .view::<TodoState>("todos", todo_reducer)
    .view::<StatsState>("stats", stats_reducer)
    .view::<AuditLog>("audit", audit_reducer)
    .open()?;
```

Views are independent:
- Each has its own snapshot file (`views/todos.snapshot.json`, etc.)
- Each tracks its own offset into the log
- Refreshing one view doesn't affect others
- Rebuilding one view doesn't touch others

This is the "same data, different lenses" pattern. The event log is the single source of truth. Views are derived projections.

## 4. Rotation and Archival

As events accumulate, `app.jsonl` grows. Rotation compresses the active log into `archive.jsonl.zst` and truncates the active log.

### What Happens During Rotation

```
Before:
  app.jsonl          = 5 MB (10,000 events)
  archive.jsonl.zst  = 2 MB (previous rotations)

rotate()

After:
  app.jsonl          = 0 bytes (truncated)
  archive.jsonl.zst  = 3 MB (previous + new frame appended)
  views/*.snapshot   = updated (offsets reset to 0)
```

Step by step:

1. All registered views are refreshed (so snapshots are up to date)
2. The contents of `app.jsonl` are compressed and appended as a new zstd frame to the archive
3. `app.jsonl` is truncated to zero bytes
4. All view snapshot offsets are reset to 0 (since the active log is now empty)

### Auto-Rotation

Configure `max_log_size` to trigger rotation automatically when the active log exceeds a threshold:

```rust
let mut log = EventLog::builder("./data")
    .max_log_size(10_000_000)  // rotate at ~10 MB
    .view::<Counter>("counter", count_reducer)
    .open()?;
```

Auto-rotation triggers on `append()` when the log exceeds the threshold, and also on `open()` if the log is already oversized.

### Choosing a Threshold

- **1-10 MB**: Good for most applications. Keeps startup fast.
- **50-100 MB**: Fine if you have many large events and don't mind slower cold starts.
- **0 (disabled)**: Manual rotation only. Use `log.rotate()` when you decide.

## 5. Schema Evolution

Event logs are append-only — you never modify past events. Schema changes happen at the reducer level.

### Adding New Event Types

Just add a new match arm to your reducer. Old events are unaffected. The wildcard `_ => {}` arm means old reducers already ignore unknown types.

### Changing State Shape

When you need to add a field to your state:

1. Add the field to your state struct with a default (using `#[serde(default)]` or `Default` impl)
2. Update the reducer to populate the new field
3. Rebuild the view: `view.rebuild(&log)?`

The rebuild replays the full history through the updated reducer, producing state with the new shape.

### Changing Event Semantics

If the meaning of an event changes, introduce a new event type rather than changing the existing one. Old events with the old type keep their original semantics; new events use the new type.

```rust
// Don't change what "user_updated" means.
// Instead, introduce "user_profile_updated" with new semantics.
```

### Deprecated Events

If an event type is no longer emitted, you can either:
- Keep the match arm (harmless, handles old events correctly)
- Remove the match arm (the `_ => {}` wildcard handles it)

Both are fine. The log still contains the old events, and `read_full()` will still return them.

## 6. Crash Safety

eventfold is designed to handle crashes gracefully.

### What's Guaranteed

- **Events are durable after `append()` returns.** Each append flushes and syncs to disk.
- **Snapshots are atomic.** Written to a `.tmp` file, synced, then renamed. A crash mid-write leaves the old snapshot intact.
- **Partial lines are skipped.** If a crash interrupts an append mid-write, the incomplete line is detected and ignored on the next read.
- **Archive appends are safe.** Each rotation appends a complete zstd frame. Partial frames at the end are handled by the decoder.

### What's Not Guaranteed

- **No concurrent writers.** eventfold assumes a single process. Multiple writers will corrupt the log.
- **No fsync on the directory.** On some filesystems, a crash after rename could theoretically lose the rename. In practice, this is extremely rare on modern filesystems.
- **Snapshot loss requires rebuild.** If both the snapshot and its `.tmp` are lost (extremely unlikely), the view rebuilds from the full log on next refresh.

### Recovery

Recovery is automatic. On the next `refresh()`:

1. The snapshot is loaded. If corrupt or missing, a full replay is triggered.
2. The snapshot's hash is verified against the log. If mismatched, a full replay is triggered.
3. Partial lines at the end of `app.jsonl` are silently skipped.

No manual intervention is needed.

## 7. Debugging

### Inspecting the Active Log

The active log is plain JSONL — one JSON object per line:

```bash
# View all events
cat data/app.jsonl | jq .

# Count events
wc -l data/app.jsonl

# Filter by type
cat data/app.jsonl | jq 'select(.type == "todo_added")'

# View the last 5 events
tail -5 data/app.jsonl | jq .
```

### Inspecting Snapshots

Snapshots are JSON files with three fields:

```bash
cat data/views/todos.snapshot.json | jq .
# {
#   "state": { "items": [...], "next_id": 3 },
#   "offset": 1284,
#   "hash": "a3f2e1b09c4d..."
# }
```

- `state`: The derived state at the time of the snapshot
- `offset`: Byte offset into `app.jsonl` after the last consumed event
- `hash`: xxh64 hash of the last event line (for integrity checking)

### Inspecting the Archive

The archive is concatenated zstd frames containing JSONL:

```bash
# Decompress and view
zstd -d data/archive.jsonl.zst --stdout | jq .

# Count archived events
zstd -d data/archive.jsonl.zst --stdout | wc -l
```

### Forcing a Rebuild

Delete the snapshot file and refresh:

```bash
rm data/views/todos.snapshot.json
# Next refresh() will replay the full history
```

Or programmatically:

```rust
view.rebuild(&log)?;
```

## 8. Limitations

Be aware of these constraints when evaluating eventfold for your use case:

- **Single-process only.** No locking, no concurrent writers. If you need multi-process access, put eventfold behind a server.
- **No ad-hoc queries.** You can't query events by field without writing a reducer or iterating manually. If you need flexible queries, use a database.
- **Reducers must be deterministic.** If your reducer uses random values, timestamps, or I/O, views won't rebuild correctly.
- **Memory-bound state.** The entire derived state lives in memory. If your state is gigabytes, eventfold isn't the right tool.
- **No built-in encryption.** Events are stored as plain text. If you need encryption, encrypt at the application layer before appending.
- **Replay cost.** A full rebuild replays every event ever recorded. With millions of events and a complex reducer, this can take seconds or minutes.
- **No event deletion.** Events are immutable and append-only. To "delete" data, append a compensating event (e.g., `user_deleted`) and handle it in your reducer.
