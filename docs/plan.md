# eventfold: A Lightweight Event Sourcing System in Rust

## Overview

A minimal, append-only event log with derived views. The core idea: your application state is always a function of the log. Views are materialized by folding events through reducers, with snapshots for incremental performance.

```
state = events.reduce(reducer, initial_state)
```

Three concepts. Events (the log). Reducers (pure functions). Views (derived JSON documents with snapshots).

---

## Core Architecture

```
data/
  archive.jsonl.zst          # all closed log history, zstd compressed, append-only
  app.jsonl                  # active log, plain text, append-only
  views/
    todos.snapshot.json      # {"state": {...}, "offset": 12840, "hash": "a3f2..."}
    stats.snapshot.json
    users.snapshot.json
```

Only two data files ever exist: the compressed archive and the active log. The archive grows via zstd frame concatenation — each rotation appends a new compressed frame. Decompression streams through all frames transparently as one continuous sequence.

Snapshots only track a byte offset into `app.jsonl`. Any view that has been refreshed has already consumed everything in the archive. Only brand new views (or rebuilds) need to touch the archive at all.

### Write Path
1. Serialize event as JSON
2. Append line to `app.jsonl`
3. Flush/sync
4. Return byte offset of the appended event

### Read Path (per view)
1. Load snapshot: `(state, byte_offset, hash)`
2. If no snapshot exists (new view / rebuild):
   a. Stream-decompress `archive.jsonl.zst` line by line, folding each event through reducer
   b. Then read `app.jsonl` from byte 0, continuing to fold
   c. Save snapshot, return state
3. If snapshot exists (normal refresh):
   a. Seek to byte offset in `app.jsonl`
   b. Read remaining events, folding through reducer
   c. If new events were processed, save snapshot
   d. Return state

---

## Crate Structure

```
eventfold/
├── Cargo.toml
├── README.md
├── src/
│   ├── lib.rs            # public API re-exports
│   ├── log.rs            # EventLog: append, read, file management
│   ├── event.rs          # Event type, serialization
│   ├── view.rs           # View<S>: reducer, snapshot, incremental refresh
│   ├── snapshot.rs       # Snapshot persistence: load/save/delete
│   └── archive.rs        # Compression: rotate active log into zstd archive
├── tests/
│   ├── common/
│   │   └── mod.rs          # shared test helpers (fixtures, temp dirs, dummy reducers)
│   ├── event_tests.rs      # serialization round-trips, edge cases
│   ├── log_tests.rs        # append, read, offsets, edge cases
│   ├── snapshot_tests.rs   # save, load, delete, atomic writes, corruption
│   ├── view_tests.rs       # refresh, rebuild, incremental, multi-view
│   ├── rotation_tests.rs   # rotation lifecycle, auto-rotation, archive integrity
│   ├── integrity_tests.rs  # hash verification, corruption recovery
│   ├── crash_safety.rs     # simulated crash at various points in the lifecycle
│   └── props.rs            # property-based tests (reducer determinism, rotation invariance)
├── examples/
│   ├── todo_cli.rs         # minimal CLI todo app
│   ├── multi_view.rs       # same log, multiple views
│   ├── rebuild.rs          # changing a reducer and rebuilding
│   ├── rotation.rs         # manual and auto rotation
│   ├── time_travel.rs      # replaying to a specific point, inspecting historical state
│   └── notes_cli.rs        # slightly richer CLI app: tagged notes with search view
├── examples-leptos/
│   └── todo-app/           # full Leptos SSR web app using eventfold
│       ├── Cargo.toml
│       ├── src/
│       │   ├── main.rs
│       │   ├── app.rs          # root Leptos component, routes
│       │   ├── state.rs        # event types, reducers, view definitions
│       │   ├── server.rs       # server functions wrapping eventfold
│       │   └── components/
│       │       ├── todo_list.rs
│       │       ├── todo_item.rs
│       │       └── stats.rs    # live stats view (demonstrates multi-view)
│       └── README.md           # setup instructions, what this demonstrates
└── docs/
    └── guide.md          # longer-form concepts and best practices
```

---

## Types

### Event

```rust
use serde::{Serialize, Deserialize};
use serde_json::Value;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
    pub ts: u64,
}
```

Events are stored as single JSON lines in the log file. The `data` field is intentionally untyped (`serde_json::Value`) — the log has no opinion about event shapes. Reducers give events meaning.

### Snapshot

```rust
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot<S> {
    pub state: S,
    pub offset: u64,
    pub hash: String,
}
```

`offset` is the byte position in `app.jsonl` after the last event consumed. `hash` is a hex-encoded hash (xxhash) of the raw bytes of the last event line processed, used as a corruption check. The offset always refers to the active log — any snapshot that exists has already consumed everything in the archive.

### Reducer

```rust
pub trait Reducer {
    type State: Serialize + DeserializeOwned + Default + Clone;

    fn reduce(state: Self::State, event: &Event) -> Self::State;
}
```

Alternatively, use a simpler function pointer approach:

```rust
type ReduceFn<S> = fn(S, &Event) -> S;
```

Start with the function pointer. Introduce the trait if more structure is needed later.

### View

```rust
pub struct View<S> {
    name: String,
    reducer: ReduceFn<S>,
    snapshot_path: PathBuf,
    state: S,
    offset: u64,
    hash: String,
}
```

A view owns its reducer function, its current in-memory state, and knows where its snapshot lives on disk. It is the primary interface for reading derived state.

### EventLog

```rust
pub struct EventLog {
    dir: PathBuf,
    log_path: PathBuf,       // app.jsonl
    archive_path: PathBuf,   // archive.jsonl.zst
    file: File,              // handle to app.jsonl, opened in append mode
    views_dir: PathBuf,
}
```

The event log manages a directory containing the active log, the compressed archive, and the views directory.

---

## Implementation Plan

### Phase 1: The Log

Implement `EventLog` with basic operations:

- **`open(dir: &str) -> io::Result<Self>`**
  - Create directory if it doesn't exist
  - Create `views/` subdirectory if it doesn't exist
  - Open or create `app.jsonl` in append mode
  - Note existence of `archive.jsonl.zst` (may not exist yet)

- **`append(event: &Event) -> io::Result<u64>`**
  - Serialize event to JSON string
  - Append `json + "\n"` to `app.jsonl`
  - Flush/sync
  - If file size exceeds `max_log_size`, call `rotate()` (blocks)
  - Return byte offset before the write

- **`read_from(offset: u64) -> io::Result<impl Iterator<Item = (Event, u64, String)>>`**
  - Open `app.jsonl` for reading, seek to byte offset
  - Return an iterator that yields `(event, next_byte_offset, line_hash)` tuples
  - Used by views during normal incremental refresh

- **`read_full() -> io::Result<impl Iterator<Item = (Event, String)>>`**
  - If `archive.jsonl.zst` exists, stream-decompress it and yield events line by line
  - Then open `app.jsonl` from byte 0 and continue yielding events
  - Used by new views or rebuilds that need the full history
  - Returns `(event, line_hash)` — no offset tracking needed until we reach `app.jsonl`

- **`rotate() -> io::Result<()>`**
  - Refresh all registered views (ensures every snapshot is up to date)
  - Compress `app.jsonl` as a zstd frame
  - Append the compressed frame to `archive.jsonl.zst` (create if first rotation)
  - Truncate `app.jsonl`
  - Reset all view snapshot offsets to 0
  - Blocks until complete

### Phase 2: Snapshots

Implement snapshot persistence:

- **`save<S: Serialize>(path: &Path, snapshot: &Snapshot<S>) -> io::Result<()>`**
  - Serialize snapshot to JSON
  - Write atomically: write to `.tmp` file, then rename (prevents corruption on crash)

- **`load<S: DeserializeOwned>(path: &Path) -> io::Result<Option<Snapshot<S>>>`**
  - Read file, deserialize
  - Return `None` if file doesn't exist

- **`delete(path: &Path) -> io::Result<()>`**
  - Remove snapshot file (triggers full rebuild on next refresh)

### Phase 3: Views

Implement `View<S>`:

- **`new(name: &str, reducer: ReduceFn<S>, views_dir: &Path) -> Self`**
  - Set snapshot path to `views_dir/{name}.snapshot.json`
  - Initialize with default state, offset 0

- **`refresh(&mut self, log: &EventLog) -> io::Result<&S>`**
  - Load snapshot from disk if not already loaded
  - If no snapshot exists, use `log.read_full()` to replay entire history (archive + active log)
  - Otherwise, use `log.read_from(self.offset)` to read only new events from active log
  - If no new events: return current state immediately (no disk write)
  - Fold each event through reducer
  - Update offset and hash to reflect position after last event consumed
  - Write snapshot to disk (always — we did work, so save it)
  - Return reference to current state

Snapshotting is always a side effect of refresh, never a separate operation. If we had to fold events, we write the snapshot. If we didn't, we don't. No configuration, no "every N events" policy. The snapshot is a cache — you update it when the underlying data changes.

- **`state(&self) -> &S`**
  - Return reference to current in-memory state (no refresh, just return what we have)

- **`rebuild(&mut self, log: &EventLog) -> io::Result<&S>`**
  - Delete snapshot, reset offset to 0, reset state to default
  - Call refresh (replays full log)

### Phase 4: Integrity Check

On snapshot load, optionally verify the hash:

- Read the event at the stored offset position minus one event (tricky with variable-length lines)
- Simpler approach: store the hash of the *last line processed* and on refresh, if offset > 0, read the line just before offset and verify its hash matches
- If mismatch, log a warning and trigger full rebuild

Alternatively, simpler: just store a hash of `(offset, file_size_at_snapshot_time)` and if the file is smaller than expected, rebuild. This catches truncation. For full integrity, rebuild is always safe and cheap enough for the target use case.

### Phase 5: Convenience API

Views must be registered with the log so that `rotate()` can refresh them all before archiving. The registry pattern is the primary interface:

```rust
let mut app = eventfold::EventLog::builder("./data")
    .max_log_size(10_000_000)   // auto-rotate on open if exceeded
    .view::<TodoState>("todos", todo_reducer)
    .view::<StatsState>("stats", stats_reducer)
    .open()?;                    // opens log, registers views, auto-rotates if needed

app.append(&Event::new("todo_added", json!({"text": "buy milk"})))?;
app.refresh_all()?;

let todos = app.view::<TodoState>("todos")?;
println!("{:?}", todos);
```

The builder registers views before opening, so auto-rotation on open can refresh them all. After open, `refresh_all()` brings every view up to date in one call. Individual views can also be refreshed independently.

### Phase 6: Log Rotation / Archival

The log is never compacted or rewritten. Instead, the active log is periodically compressed and appended to a single archive file.

**Rotation process (`rotate()`):**

1. Refresh all registered views — every snapshot now reflects everything in `app.jsonl`
2. Compress `app.jsonl` as a zstd frame
3. Append the compressed frame to `archive.jsonl.zst` (create if first rotation)
4. Truncate `app.jsonl`
5. Reset all view snapshot offsets to 0

After step 1, every view has consumed the entire active log. Resetting offsets to 0 is safe — their state is complete, the events they consumed are now in the archive. No temporary files, no race conditions, no background threads.

This means `EventLog` must know about all views in order to refresh them before rotating. The registry pattern is therefore required — views must be registered with the log, not standalone.

- **Auto-rotation on append:** When `max_log_size` is configured, `append()` checks the active log size after writing. If over threshold, rotation happens inline — the append blocks until rotation completes. For the target use case (small apps, low write volume), the occasional pause is negligible.

- **Manual rotation:** `log.rotate()` is always available. Blocks until complete.

- **Disabling auto-rotation:** Set `max_log_size(0)` to disable. The user manages rotation themselves via `log.rotate()`.

- **zstd frame concatenation:** zstd natively supports concatenated frames. Each rotation appends a new frame to the archive. On decompression, all frames stream through transparently as one continuous sequence. The archive itself is append-only — just like the log.

- **Impact on normal operation:** None. Views only read `app.jsonl` during incremental refresh. The archive is never touched during normal reads.

- **Impact on new views / rebuilds:** A new view (or a rebuild after a reducer change) streams through `archive.jsonl.zst` then reads the active log, folding everything through the reducer. Slow but happens once, producing a snapshot that subsequent refreshes build on incrementally.

- **Optional future: archive pruning.** If all view snapshots exist and are up to date, the archive is technically redundant — it's only needed for building new views or disaster recovery. Deleting it is an explicit, opt-in operation. The user accepts that new views can only be built from the active log forward.

---

## Testing Strategy

All tests use temporary directories (`tempfile::tempdir()`) so they're isolated and clean up automatically.

### Test Helpers (`tests/common/mod.rs`)

Shared utilities to keep tests concise and consistent:

- **`test_dir() -> TempDir`** — creates a fresh temporary directory for each test
- **`dummy_event(event_type: &str) -> Event`** — creates an event with minimal data and a fixed timestamp
- **`counter_reducer(state: u64, event: &Event) -> u64`** — simplest possible reducer: counts events. Good for testing mechanics without business logic noise.
- **`todo_reducer`** — the todo reducer from the example. Used for tests that need a realistic state shape.
- **`append_n(log: &mut EventLog, n: usize)`** — append N dummy events. Avoids boilerplate in tests that just need "a log with stuff in it."
- **`assert_events_eq(events: Vec<Event>, expected_types: Vec<&str>)`** — verify a sequence of events matches expected types in order

### Unit Tests

**Event serialization (`event_tests.rs`)**
- Event round-trips through JSON (serialize → deserialize → equal)
- Timestamp and event type preserved correctly
- Arbitrary `serde_json::Value` data survives serialization
- Events with missing fields fail gracefully on deserialization
- Events with special characters in data (unicode, embedded newlines, escaped quotes) round-trip correctly
- Event serialized to exactly one line (no embedded newlines in the JSON output)

**Snapshot persistence (`snapshot_tests.rs`)**
- Save and load round-trip produces identical state
- Load from nonexistent file returns `None`
- Atomic write: if process crashes mid-save, old snapshot survives (simulate by checking `.tmp` file handling)
- Delete removes the file, subsequent load returns `None`
- Snapshot with various state types (empty struct, nested structs, large state)
- Snapshot with offset 0 (fresh after rotation)
- Snapshot with large offset value

**Log reading/writing (`log_tests.rs`)**
- Append single event, read it back
- Append multiple events, read all back in order
- `read_from(0)` returns all events
- `read_from(offset)` after N events returns only events after N
- Byte offsets are correct — seeking to a returned offset yields the next event
- Empty log returns empty iterator
- Events with special characters (unicode, newlines in string values, escaped quotes) survive round-trip
- Line hash is deterministic — same event bytes produce same hash
- Append to existing log (close and reopen, verify previous events still readable, new events append correctly)
- `read_full()` with no archive, just active log → reads active log from start
- `read_full()` with archive + active log → reads archive then active, all events in correct order
- `read_full()` with archive, empty active log → reads only archive events
- `read_full()` with neither archive nor events → empty iterator

### Integration Tests

**View lifecycle (`view_tests.rs`)**
- Fresh view with no snapshot replays from empty log → default state
- Fresh view with no snapshot replays from populated log → correct state
- Refresh with no new events returns current state, does not write snapshot
- Refresh with new events folds them in, writes snapshot
- Subsequent refresh loads snapshot, only processes new events
- `state()` returns current in-memory state without I/O
- `rebuild()` deletes snapshot, replays full log, produces same state as fresh view
- View refresh is idempotent — calling refresh twice with no new events produces same state, no extra disk writes

**Multiple views over same log**
- Two views with different reducers produce different states from same events
- Each view maintains independent snapshots
- Refreshing one view does not affect the other
- Adding events and refreshing both views produces correct independent states
- New view registered after events already exist → first refresh replays full history correctly

**Snapshot correctness**
- Append N events, refresh view, kill process (drop without clean shutdown), reopen, refresh again → state is correct
- Append events, refresh, append more, refresh → state matches full replay
- Delete snapshot file manually, refresh → full rebuild produces correct state
- Corrupt snapshot file (truncate, garble bytes) → detected, triggers rebuild
- Snapshot with offset beyond end of file → detected, triggers rebuild

**Rotation (`rotation_tests.rs`)**
- Append events, rotate → `app.jsonl` is empty, `archive.jsonl.zst` exists
- After rotation, all view snapshots have offset 0
- After rotation, view state is unchanged (still reflects pre-rotation events)
- Append more events after rotation, refresh view → new events folded in correctly
- Multiple rotations → archive contains all events from all rotations
- `read_full()` after multiple rotations yields all events in correct order
- New view created after rotation → replays archive + active log, produces correct state
- Rotation with no events in active log → no-op (or empty frame, either is fine)

**Auto-rotation on append (`rotation_tests.rs`)**
- Set `max_log_size` to small value, append events until threshold crossed → rotation triggered automatically
- After auto-rotation, all view states are consistent
- After auto-rotation, subsequent appends go to fresh `app.jsonl`
- With `max_log_size(0)`, no auto-rotation occurs regardless of file size
- Auto-rotation mid-stream: append 100 events with a small threshold → multiple rotations may fire, final state matches full sequential replay

**Integrity checks (`integrity_tests.rs`)**
- Valid snapshot with matching hash → accepted
- Snapshot with mismatched hash → detected, triggers rebuild
- Snapshot offset beyond end of file (e.g. after manual truncation) → detected, triggers rebuild
- Snapshot from before a rotation (offset points into old data) → handled gracefully
- Manually edited `app.jsonl` (inserted line in middle) → hash mismatch detected on next refresh
- Empty `app.jsonl` with snapshot at offset > 0 (someone truncated the log) → detected, triggers rebuild

### Crash Safety Tests (`crash_safety.rs`)

Simulate crashes at various points by manipulating files directly:

- **Crash during append:** Write partial line to `app.jsonl` (no trailing newline). On reopen, partial line is either skipped or detected. No data corruption. Previously committed events are intact.
- **Crash during snapshot write:** The `.tmp` file exists but was never renamed. On reopen, old snapshot is used (`.tmp` ignored). State rebuilt from old snapshot + new events.
- **Crash during rotation (after compress, before truncate):** `archive.jsonl.zst` has new frame, `app.jsonl` still has old events. On reopen, events would be duplicated between archive and active log. Detect via hash/offset and handle: either truncate on open or rebuild views from `read_full()`.
- **Crash during rotation (after truncate, before snapshot offset reset):** `app.jsonl` is empty, snapshots still point to old offsets. On reopen, offset beyond EOF detected, triggers rebuild from archive.

These tests don't need actual process crashes — they set up the filesystem state that would result from a crash at each point, then verify the system recovers correctly on open/refresh.

### Property-Based Tests (`props.rs`)

Use `proptest` or `quickcheck`:

- **Reducer determinism:** For any sequence of random events, replaying the full log always produces the same state as incremental refreshes with arbitrary snapshot points
- **Rotation invariance:** For any sequence of events with rotations inserted at random points, the final state is identical to replaying all events without any rotation
- **Snapshot equivalence:** Deleting all snapshots and rebuilding every view produces identical state to the incrementally maintained state
- **Ordering:** Events read back from `read_full()` after arbitrary rotations are in the exact order they were appended
- **Multi-view consistency:** For any sequence of events, every view's state after `refresh_all()` matches what you'd get from a fresh replay with that view's reducer

### Performance / Stress Tests

Not for CI, but useful for validation:

- Append 1M events, verify incremental refresh is fast (should only process new events)
- Append 1M events, rotate, create new view, verify full replay completes in reasonable time
- Verify snapshot file size stays proportional to state size, not event count
- Verify `read_from` with a late offset doesn't scan the whole file (seek is O(1))
- Multiple rotations with 100K events each, verify archive decompresses correctly end-to-end
- Measure rotation time as a function of active log size (should be roughly linear)

---

## Documentation

### README.md

The README is the primary entry point. It should convey the core idea in under 30 seconds and get someone to a working example in under 2 minutes.

**Structure:**

1. **One-liner:** "Your application state is a fold over an event log."
2. **What it is:** 3-4 sentences. Append-only event log, derived views via reducers, snapshots for performance, single directory, no infrastructure.
3. **Quick example:** Complete, runnable code. The todo app — define state, define reducer, open log, append events, read state. ~30 lines.
4. **Core concepts:** Brief section explaining the three primitives — events, reducers, views — in 2-3 sentences each.
5. **Installation:** `cargo add eventfold`
6. **Features section:**
   - Append-only event log (JSONL)
   - Derived views via reducer functions
   - Incremental snapshots (only process new events)
   - Automatic log rotation with zstd compression
   - Integrity checking via hashing
   - Crash-safe (atomic snapshot writes, graceful recovery from partial writes)
   - Zero infrastructure — just files in a directory
7. **When to use / when not to use:** Small apps, prototypes, tools, CLIs, embedded state. Not for: high-concurrency, multi-process, distributed systems.
8. **Link to full docs**

### API Documentation (rustdoc)

Every public type, method, and function gets a doc comment. Follow Rust conventions: first line is a summary, then a blank line, then details. Include `# Examples` with runnable doctests for all public methods.

**`EventLog`**
- Module-level: explain the overall system, link to README concepts
- `builder()`: explain the builder pattern, all configurable options
- `append()`: explain auto-rotation trigger, what the returned offset means, that it may block if rotation is triggered
- `read_from()`: when you'd use this directly (usually you wouldn't — views handle it)
- `read_full()`: when you'd use this directly (rebuilds, debugging)
- `rotate()`: what it does step by step, that it blocks, that it refreshes all views first
- `refresh_all()`: refreshes every registered view, snapshots as side effect
- `view()`: access a registered view's current state

**`EventLogBuilder`**
- `max_log_size()`: what the default is, what 0 means (disabled), units (bytes)
- `view()`: register a view by name and reducer, can be chained
- `open()`: what it creates on disk, what happens if directory already exists, when auto-rotation fires

**`Event`**
- How to construct: `Event::new(type, data)`
- What `ts` is (unix timestamp, auto-populated)
- That `data` is arbitrary JSON — the log doesn't validate it
- That the JSON serialization is guaranteed to be a single line (no pretty printing)

**`View`**
- `refresh()`: explain the snapshot-as-side-effect behavior, the incremental read path, that it may trigger a full replay if no snapshot exists
- `state()`: returns current in-memory state, no I/O
- `rebuild()`: deletes snapshot, replays full history (archive + active log)

**`Snapshot`**
- Internal type, but document what `offset` and `hash` mean for users who inspect snapshot files
- Document that snapshots are written atomically (write to `.tmp`, rename)

**`ReduceFn`**
- Document the signature, explain it's a pure function, link to examples
- Explain that unknown event types should be ignored (forward compatibility)
- Explain that the function receives owned state and returns owned state

### Guide / Concepts (`docs/guide.md`)

Longer-form documentation for users who want to understand the system deeply:

1. **How it works** — the lifecycle of an event from append through to view state. Walk through the data flow with ASCII diagrams showing the write path, read path, and rotation path.

2. **Writing reducers** — best practices:
   - Keep them pure: no I/O, no side effects, no randomness
   - Always handle the `_ =>` case (ignore unknown event types) for forward compatibility
   - Prefer owned mutation (`fn(mut state, event) -> state`) over clone-and-modify
   - Keep state shapes flat when possible — deeply nested state is harder to debug
   - Test reducers in isolation: they're just functions, feed them events and assert state

3. **Multiple views** — why you'd want them, how they're independent, the mental model of "same log, different lenses." Concrete examples: a current-state view vs. an aggregation/analytics view vs. a search index view.

4. **Rotation and archival** — what happens during rotation step by step, why auto-rotation exists, how to configure `max_log_size`, what the archive file is, why it uses zstd, that rotation blocks. Include guidance on choosing `max_log_size` — smaller means more frequent pauses but smaller active logs; larger means less frequent but longer pauses.

5. **Schema evolution** — the critical guide for real-world usage:
   - **New event type:** Add it to the reducer. Old events are ignored. No migration.
   - **Changed state shape:** Update the reducer, delete the snapshot, `rebuild()`. The log doesn't change.
   - **Changed event semantics:** Append a migration/correction event. Handle it in the reducer. The old events remain as historical record.
   - **Deprecated event type:** Just stop emitting it. The reducer still handles old ones in the archive.
   - **Example walkthrough:** A todo app evolving from v1 (just text) to v2 (text + priority) to v3 (text + priority + tags).

6. **Crash safety guarantees** — what the system guarantees and what it doesn't:
   - **Appended events are durable** after `append()` returns (flushed and synced to disk)
   - **Snapshots are atomic** (write to `.tmp`, rename). A crash mid-write leaves the old snapshot intact.
   - **Partial writes to the log** (crash mid-append) result in a truncated last line. On next read, the partial line is detected and skipped. All previously committed events are intact.
   - **Crash during rotation** may leave the system in an intermediate state (events in both archive and active log, or snapshots with stale offsets). On next open/refresh, the system detects the inconsistency and rebuilds from the archive.
   - **What is NOT guaranteed:** if the OS or filesystem lies about fsync, all bets are off (this is true of every database). eventfold trusts the OS to honor flush/sync.

7. **Debugging** — practical tips:
   - Inspect the log: `cat app.jsonl` or `cat app.jsonl | jq .`
   - Inspect snapshots: `cat views/todos.snapshot.json | jq .`
   - Inspect the archive: `zstd -d archive.jsonl.zst --stdout | head -20`
   - Replay to a specific point: read events one by one in a test, stop where you want
   - Use `rebuild()` to start a view fresh after a reducer change
   - If all else fails: delete all snapshots, they're just caches

8. **Limitations** — be honest about what this is and isn't:
   - Single-process, single-writer only
   - Fully synchronous — `append()` may block during auto-rotation
   - Not a database — no ad-hoc queries, no indexes (beyond what your reducer builds)
   - Not for high write throughput — every append flushes to disk
   - Log grows forever (archive compresses, but never deletes)
   - No built-in encryption or access control
   - No networking — this is an embedded library

### Examples (`examples/`)

Each example should be a complete, runnable program with comments explaining what's happening and why. Include expected output in comments.

- **`todo_cli.rs`** — minimal CLI todo app. The "hello world" of eventfold. Shows: define state, define reducer, open log with builder, append events, refresh, print state. This should be ~50 lines and immediately understandable.

- **`multi_view.rs`** — same event log with two views: one for current todo state, one for statistics (total added, total completed, completion rate). Shows: same events, different reducers, independent snapshots, each view only cares about the events relevant to it.

- **`rebuild.rs`** — demonstrates changing a reducer and rebuilding a view. Shows: append events with v1 reducer (todos with just text), update reducer to v2 (todos with text + priority, defaulting to "normal"), call `rebuild()`, verify new state shape includes the priority field.

- **`rotation.rs`** — demonstrates manual and auto rotation. Shows: configure `max_log_size` to a small value, append enough events to trigger rotation, list directory contents to show `archive.jsonl.zst` appeared, append more, verify all state is continuous.

- **`time_travel.rs`** — demonstrates replaying to a specific point. Shows: append 20 events, then read events one by one and reduce manually, stopping at event 10. Print the state at that point. Demonstrates the "debugging superpower" of event sourcing.

- **`notes_cli.rs`** — a slightly richer CLI app. A note-taking tool where you can add notes with tags, list notes, filter by tag, and see tag statistics. Two views: `notes_view` (current notes with tags) and `tags_view` (tag counts, most-used tags). Demonstrates a more realistic state shape with multiple entity types.

### Leptos Web Application (`examples-leptos/todo-app/`)

A complete, working Leptos SSR web application that uses eventfold as its entire data layer. This is the flagship example — it demonstrates that eventfold can power a real web app, not just CLI scripts.

**What it demonstrates:**
- eventfold as the sole persistence layer for a web application (no database)
- Server functions that wrap eventfold operations
- Multiple views powering different parts of the UI
- The full lifecycle: create, complete, delete todos + live statistics
- How little code is needed when your "database" is just a reducer

**Architecture:**

```
Browser (Leptos client)
  ↕ server functions (HTTP)
Leptos server
  ↕ eventfold API
data/
  archive.jsonl.zst
  app.jsonl
  views/
    todos.snapshot.json
    stats.snapshot.json
```

**State and events (`state.rs`):**

```rust
// Events the app can produce
// "todo_added"    { "id": "uuid", "text": "...", "created_at": 1234 }
// "todo_toggled"  { "id": "uuid" }
// "todo_deleted"  { "id": "uuid" }

#[derive(Default, Clone, Serialize, Deserialize)]
struct TodoState {
    items: Vec<Todo>,
}

#[derive(Default, Clone, Serialize, Deserialize)]
struct StatsState {
    total_created: u64,
    total_completed: u64,
    total_deleted: u64,
}

fn todo_reducer(mut state: TodoState, event: &Event) -> TodoState {
    match event.event_type.as_str() {
        "todo_added" => { /* push new item */ }
        "todo_toggled" => { /* flip done flag */ }
        "todo_deleted" => { /* retain all except id */ }
        _ => {}
    }
    state
}

fn stats_reducer(mut state: StatsState, event: &Event) -> StatsState {
    match event.event_type.as_str() {
        "todo_added" => state.total_created += 1,
        "todo_toggled" => state.total_completed += 1,  // simplified
        "todo_deleted" => state.total_deleted += 1,
        _ => {}
    }
    state
}
```

**Server integration (`server.rs`):**

```rust
// EventLog lives in server state, shared via Arc<Mutex<>>
// Each server function locks, operates, unlocks

#[server]
async fn add_todo(text: String) -> Result<(), ServerFnError> {
    let log = use_eventfold()?;  // extract from server context
    let mut log = log.lock().unwrap();
    log.append(&Event::new("todo_added", json!({
        "id": Uuid::new_v4().to_string(),
        "text": text,
        "created_at": now(),
    })))?;
    log.refresh_all()?;
    Ok(())
}

#[server]
async fn get_todos() -> Result<TodoState, ServerFnError> {
    let log = use_eventfold()?;
    let mut log = log.lock().unwrap();
    log.refresh_all()?;
    Ok(log.view::<TodoState>("todos")?.clone())
}

#[server]
async fn get_stats() -> Result<StatsState, ServerFnError> {
    let log = use_eventfold()?;
    let mut log = log.lock().unwrap();
    // only refresh the stats view, not everything
    Ok(log.view::<StatsState>("stats")?.clone())
}
```

**UI components:**

- **`app.rs`** — root component. Sets up routes: `/` for the todo list. Initializes the eventfold log in server context on startup via Leptos's server state / provide_context.
- **`todo_list.rs`** — fetches todos via `get_todos` server function. Renders the list. Has an input form that calls `add_todo`. Each item can be toggled or deleted. Uses Leptos actions and resources for reactivity.
- **`todo_item.rs`** — single todo row. Toggle checkbox calls `toggle_todo` server function. Delete button calls `delete_todo`. Optimistic UI optional.
- **`stats.rs`** — sidebar or footer component showing live stats from the stats view. Demonstrates that multiple views can power different parts of the same page independently.

**Setup in `main.rs`:**

```rust
#[tokio::main]
async fn main() {
    // Initialize eventfold
    let log = eventfold::EventLog::builder("./data")
        .max_log_size(10_000_000)
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .expect("failed to open event log");

    let log = Arc::new(Mutex::new(log));

    // Provide to Leptos server context
    // ... standard Leptos SSR setup with Actix or Axum ...
}
```

**README for the example:**

Should explain:
1. What this demonstrates (eventfold as a web app's entire data layer)
2. How to run it (`cargo leptos watch`)
3. What to look at first (`state.rs` for the data model, `server.rs` for the integration)
4. Where the data lives (`./data/` directory)
5. How to inspect state (`cat data/app.jsonl | jq .`)
6. The deliberate constraints: single-process, no multi-server deployment, and why that's fine for the target use case (personal tools, prototypes, internal apps, small teams)

---

## Dependencies

### Runtime

| Crate            | Purpose                        |
|------------------|--------------------------------|
| `serde`          | Serialization framework        |
| `serde_json`     | JSON serialization             |
| `xxhash-rust`    | Fast hashing for integrity     |
| `zstd`           | Compression for archive        |

### Dev / Test

| Crate            | Purpose                        |
|------------------|--------------------------------|
| `tempfile`       | Temporary directories for tests|
| `proptest`       | Property-based testing         |

Keep dependencies minimal. Avoid async, frameworks, or anything heavy. This is meant to be embeddable and simple.

### Leptos Example (`examples-leptos/todo-app/`)

The Leptos example is a separate crate with its own `Cargo.toml` and additional dependencies:

| Crate            | Purpose                        |
|------------------|--------------------------------|
| `eventfold`      | Path dependency to parent crate|
| `leptos`         | Reactive web framework (SSR)   |
| `leptos_actix` or `leptos_axum` | Server integration  |
| `tokio`          | Async runtime (Leptos requires it) |
| `uuid`           | Generating todo IDs            |
| `serde`          | Shared serialization           |

Note: the core `eventfold` crate remains synchronous. The Leptos example wraps it in `Arc<Mutex<>>` and calls it from async server functions. This is fine for the target use case — the lock is held only briefly during append/refresh.

---

## Quick Example: Todo Reducer

```rust
use serde::{Serialize, Deserialize};

#[derive(Default, Clone, Serialize, Deserialize)]
struct TodoState {
    items: Vec<TodoItem>,
    next_id: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct TodoItem {
    id: u64,
    text: String,
    done: bool,
}

fn todo_reducer(mut state: TodoState, event: &Event) -> TodoState {
    match event.event_type.as_str() {
        "todo_added" => {
            state.items.push(TodoItem {
                id: state.next_id,
                text: event.data["text"].as_str().unwrap().to_string(),
                done: false,
            });
            state.next_id += 1;
        }
        "todo_completed" => {
            let id = event.data["id"].as_u64().unwrap();
            if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                item.done = true;
            }
        }
        "todo_deleted" => {
            let id = event.data["id"].as_u64().unwrap();
            state.items.retain(|i| i.id != id);
        }
        _ => {} // ignore unknown events
    }
    state
}
```

This is the entire "data layer" for a todo app. No schema, no migrations, no ORM. The log file is the database. The reducer is the schema.

---

## Non-Goals

- **Concurrency across processes.** Single-process, single-writer is fine. File locks can be added later if needed.
- **Async.** Synchronous I/O is simpler and sufficient for the target use case.
- **Networking.** This is an embedded library, not a server.
- **Event validation / command layer.** Users can add this themselves. The log just stores what it's told.
- **Compaction / log rewriting.** The log is never rewritten or compacted. Old events are compressed into the archive via rotation.

---

## Open Questions

1. **Should views auto-refresh on access, or require explicit refresh calls?** Auto-refresh is more ergonomic but hides I/O. Explicit is more honest.
2. **Should `EventLog` own the views, or should views be standalone?** Resolved: `EventLog` must own/know about views because `rotate()` needs to refresh all views before archiving. The registry pattern is required.
3. **Typed events vs `serde_json::Value`?** Starting with `Value` keeps the log generic. Users can deserialize into typed enums in their reducers if they want.
4. **Snapshot frequency.** Resolved: always snapshot as a side effect of refresh if any new events were processed. No knobs.
