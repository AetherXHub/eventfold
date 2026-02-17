# Eventfold: Foundation Improvements for eventfold-es

This document outlines changes to the `eventfold` crate that prepare it to serve as the file-level primitive beneath `eventfold-es`, a multi-stream event store. The goal is to make eventfold a better building block without changing its identity — it stays embedded, synchronous, and file-backed. The async and multi-stream coordination lives upstream in eventfold-es.

---

## 1. Reader/Writer Separation

### Problem

`EventLog` currently owns both the append file handle and all read methods. This creates awkward borrowing — `View::refresh` needs `&EventLog` for reading while the log also needs `&mut self` to manage views, leading to the `std::mem::take` workaround in `refresh_all()` and `rotate()`.

More importantly, eventfold-es needs to hand out many readers (to projections, subscriptions, query handlers) while holding a single writer per stream. The current design doesn't support this.

### Proposed Design

```rust
/// Exclusive writer for a single event log file.
/// Holds an exclusive flock on app.jsonl for its lifetime.
pub struct EventWriter {
    file: File,
    log_path: PathBuf,
    archive_path: PathBuf,
    max_log_size: u64,
}

/// Cheap, cloneable reader for a single event log file.
/// Acquires shared flock per read operation.
#[derive(Clone)]
pub struct EventReader {
    log_path: PathBuf,
    archive_path: PathBuf,
}

/// Convenience wrapper that owns both. Preserves the existing
/// simple API for direct eventfold users.
pub struct EventLog {
    writer: EventWriter,
    reader: EventReader,
    views: HashMap<String, Box<dyn ViewOps>>,
}
```

### Migration Path

`EventLog` keeps its current public API by delegating to the inner writer and reader. Existing code doesn't break. New code can use `EventWriter` and `EventReader` directly.

```rust
// Existing API still works
let mut log = EventLog::builder("./data")
    .view::<Counter>("counter", count_reducer)
    .open()?;

// New API for eventfold-es
let writer = EventWriter::open("./data/streams/order-123")?;
let reader = writer.reader(); // cloneable
```

### Key Behaviors

- `EventWriter::open` creates directories, opens the file in append mode, acquires exclusive flock
- `EventReader` is `Clone + Send + Sync` — it opens new file handles per read call (as `read_from` already does today)
- `EventWriter` exposes a `.reader()` method that returns a reader pointed at the same paths
- Views and snapshots accept `&EventReader` instead of `&EventLog`

---

## 2. AppendResult with Offset and Hash

### Problem

`append()` currently returns only the byte offset where the event starts. eventfold-es needs to track the stream's version after each write — specifically the offset *after* the newline and the hash of the written line. These are the values that get checked during conditional appends.

The line hash computation already exists (`line_hash()`) but isn't used on the write path.

### Proposed Design

```rust
/// Result of a successful append operation.
pub struct AppendResult {
    /// Byte offset where the event line starts in app.jsonl.
    pub start_offset: u64,
    /// Byte offset after the trailing newline — the position
    /// where the next event would begin.
    pub end_offset: u64,
    /// xxh64 hash of the serialized event line (hex-encoded).
    pub line_hash: String,
}

impl EventWriter {
    pub fn append(&mut self, event: &Event) -> io::Result<AppendResult> {
        let start_offset = self.file.seek(SeekFrom::End(0))?;
        let json = serde_json::to_string(event)?;
        let hash = line_hash(json.as_bytes());
        writeln!(self.file, "{json}")?;
        self.file.sync_data()?;
        let end_offset = self.file.seek(SeekFrom::End(0))?;

        // auto-rotate check (unchanged)
        if self.max_log_size > 0 && end_offset >= self.max_log_size {
            self.rotate()?;
        }

        Ok(AppendResult {
            start_offset,
            end_offset,
            line_hash: hash,
        })
    }
}
```

### Usage in eventfold-es

```rust
// eventfold-es stores the result as the stream's current version
let result = writer.append(&event)?;
stream_versions.insert("order-123", StreamVersion {
    offset: result.end_offset,
    hash: result.line_hash,
});
```

---

## 3. Conditional Append

### Problem

eventfold-es needs optimistic concurrency per stream: "append this event only if no one else has written since I last read." Without this, business invariants can't be enforced. The check should happen at the file level under the exclusive lock, not in a layer above.

### Proposed Design

```rust
/// Returned when a conditional append fails due to a version conflict.
#[derive(Debug)]
pub struct AppendConflict {
    pub expected_offset: u64,
    pub actual_offset: u64,
    pub expected_hash: String,
    pub actual_hash: Option<String>,
}

impl EventWriter {
    /// Append only if the log's current state matches expectations.
    ///
    /// Checks that the file length equals `expected_offset` and that
    /// the hash of the last line matches `expected_hash`. If either
    /// check fails, returns `Err(AppendConflict)` without writing.
    ///
    /// For a new/empty stream, pass `expected_offset: 0` and
    /// `expected_hash: ""`.
    pub fn append_if(
        &mut self,
        event: &Event,
        expected_offset: u64,
        expected_hash: &str,
    ) -> Result<AppendResult, AppendConflict> {
        let current_size = self.file.seek(SeekFrom::End(0))?;

        if current_size != expected_offset {
            return Err(AppendConflict {
                expected_offset,
                actual_offset: current_size,
                expected_hash: expected_hash.to_string(),
                actual_hash: None,
            });
        }

        if expected_offset > 0 {
            let actual_hash = self.read_last_line_hash()?;
            if actual_hash != expected_hash {
                return Err(AppendConflict {
                    expected_offset,
                    actual_offset: current_size,
                    expected_hash: expected_hash.to_string(),
                    actual_hash: Some(actual_hash),
                });
            }
        }

        // Checks passed — proceed with normal append
        Ok(self.append(event)?)
    }
}
```

### Design Notes

- The offset check is the fast path — just a seek. The hash check only triggers if the offset matches, which should be the common case.
- `read_last_line_hash` reuses the logic from `read_line_hash_before` (already implemented in `log.rs`), pulled into the writer where it can operate on the held file handle.
- The error type is a struct, not an `io::Error`, so callers can inspect the conflict and implement retry logic.
- The return type should probably be `Result<AppendResult, ConditionalAppendError>` with variants for both conflicts and I/O errors. Simplified here for clarity.

---

## 4. File Locking (flock)

### Problem

eventfold currently has no file-level coordination. Two processes opening the same log directory will corrupt data. Even within eventfold-es (single process), it's good hygiene to lock the files so external tools can't interfere.

### Proposed Design

```rust
pub enum LockMode {
    /// Acquire flock locks. Default.
    Flock,
    /// No locking. For testing or known single-access scenarios.
    None,
}

impl EventWriter {
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_lock(dir, LockMode::Flock)
    }

    pub fn open_with_lock(dir: impl AsRef<Path>, lock: LockMode) -> io::Result<Self> {
        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        if matches!(lock, LockMode::Flock) {
            // Exclusive lock — blocks if another writer exists,
            // or use try_lock for non-blocking with an error
            flock(&file, FlockOp::ExclusiveNonBlocking)?;
        }

        // ...
    }
}
```

### Locking Strategy

| Operation | Lock Type | Scope |
|-----------|-----------|-------|
| `EventWriter::open` | Exclusive (`LOCK_EX`) | Held for lifetime of writer |
| `EventReader::read_from` | None (append-only is safe to read concurrently) | — |
| `EventWriter::rotate` | Already exclusive via writer lock | — |

Append-only files have a useful property: readers don't need locks. A reader that opens the file and reads up to a known offset will always see consistent data, because completed lines (with trailing newlines) are immutable once written. The partial-line detection already in the `LogIterator` handles the edge case of reading during a concurrent write.

The exclusive writer lock prevents two writers from interleaving partial lines.

### Platform Notes

- Linux/macOS: `libc::flock` or the `fs2` crate
- Windows: `LockFileEx` via the `fs2` crate
- The `fs2` crate provides cross-platform `FileExt::lock_exclusive()` / `try_lock_exclusive()` and is a single, well-maintained dependency

---

## 5. Tail / New-Event Detection

### Problem

eventfold-es needs subscriptions. Subscriptions need tailing. The lowest-level question — "are there new events after offset N?" — belongs in eventfold because it's about a single file.

### Proposed Design

Two primitives, from simplest to most capable:

#### 5a. Poll Check (Non-blocking)

```rust
impl EventReader {
    /// Returns true if the active log contains data beyond `offset`.
    /// Non-blocking — just a metadata stat call.
    pub fn has_new_events(&self, offset: u64) -> io::Result<bool> {
        Ok(fs::metadata(&self.log_path)?.len() > offset)
    }

    /// Returns the current size of the active log in bytes.
    /// Useful as a lightweight "version" check.
    pub fn active_log_size(&self) -> io::Result<u64> {
        Ok(fs::metadata(&self.log_path)?.len())
    }
}
```

This is enough for eventfold-es to build async subscriptions:

```rust
// In eventfold-es (not eventfold)
async fn subscribe(reader: EventReader, mut offset: u64) {
    loop {
        if reader.has_new_events(offset)? {
            for event in reader.read_from(offset)? {
                // dispatch to subscriber
                offset = event.next_offset;
            }
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
}
```

#### 5b. Blocking Wait (Optional, Higher Value)

```rust
impl EventReader {
    /// Block the current thread until new data appears after `offset`,
    /// or until `timeout` elapses. Returns the new file size.
    ///
    /// Uses inotify (Linux), kqueue (macOS), or polling as fallback.
    pub fn wait_for_events(
        &self,
        offset: u64,
        timeout: Duration,
    ) -> io::Result<WaitResult> { ... }
}

pub enum WaitResult {
    /// New data is available. Contains the new file size.
    NewData(u64),
    /// Timeout elapsed with no new data.
    Timeout,
}
```

This avoids busy-polling and gives eventfold-es a zero-latency notification path. The implementation can start with a polling fallback and add platform-specific watchers later.

### Design Notes

- Neither of these is async. eventfold stays synchronous. eventfold-es wraps `wait_for_events` in `spawn_blocking` or uses `has_new_events` with a timer.
- The `notify` crate is a clean cross-platform option for the blocking variant, but adds a dependency. A first pass could just use polling with exponential backoff.

---

## 6. Decouple Views from EventLog

### Problem

`View::refresh` takes `&EventLog`, coupling views to the combined reader+writer struct. eventfold-es wants to use views (and snapshots) as building blocks for its projection system, driven by `EventReader` instances.

### Proposed Change

```rust
impl<S> View<S>
where
    S: Serialize + DeserializeOwned + Default + Clone,
{
    // Before:
    // pub fn refresh(&mut self, log: &EventLog) -> io::Result<&S>

    // After:
    pub fn refresh(&mut self, reader: &EventReader) -> io::Result<&S> {
        // Same logic, but calls reader.read_from / reader.read_full
        // instead of log.read_from / log.read_full
    }

    pub fn rebuild(&mut self, reader: &EventReader) -> io::Result<&S> {
        // Same change
    }
}
```

The `ViewOps` trait changes correspondingly:

```rust
pub trait ViewOps {
    fn refresh_boxed(&mut self, reader: &EventReader) -> io::Result<()>;
    fn reset_offset(&mut self) -> io::Result<()>;
    fn view_name(&self) -> &str;
    fn as_any(&self) -> &dyn Any;
}
```

`EventLog` (the convenience wrapper) passes its internal `reader` to view methods. No external API change for existing users.

---

## Implementation Sequence

These changes build on each other. The recommended order:

| Phase | Change | Effort | Unblocks |
|-------|--------|--------|----------|
| 1 | Reader/Writer split | Medium | Everything below |
| 2 | `AppendResult` with hash | Small | Conditional append, version tracking in eventfold-es |
| 3 | Conditional append (`append_if`) | Small | Optimistic concurrency in eventfold-es |
| 4 | File locking (flock) | Small | Multi-process safety |
| 5 | `has_new_events` poll check | Trivial | Subscriptions in eventfold-es |
| 6 | Decouple views from EventLog | Medium | Projections in eventfold-es |
| 7 | `wait_for_events` blocking tail | Medium | Low-latency subscriptions |

Phases 1–5 form the minimum viable foundation. Phase 6 is needed before eventfold-es builds its projection system. Phase 7 is an optimization that can come later — poll-based tailing works fine initially.

---

## What Stays Out of Eventfold

To keep the boundary clean, the following remain the responsibility of eventfold-es:

- **Streams and stream routing** — eventfold knows about one log file; eventfold-es maps stream names to log instances
- **Async runtime** — eventfold stays synchronous; eventfold-es wraps in tokio/async-std
- **Global ordering (`$all`)** — cross-stream concern, not a single-file concern
- **Cross-stream projections** — built on top of multiple `EventReader` instances
- **Subscription management** — consumer groups, checkpointing, backpressure
- **Typed event deserialization** — the store layer maps event types to domain enums

Eventfold's job is to be a correct, efficient, lockable, tailable append-only log file. Everything above that is composition.
