# PRD 17: Blocking Tail (wait_for_events)

## Summary

Add `EventReader::wait_for_events(offset, timeout)` — a blocking primitive that suspends the calling thread until new data appears in the active log or a timeout elapses. This gives eventfold-es a zero-latency notification path without busy-polling, while keeping eventfold itself synchronous.

## Prerequisites

- PRD 12 (Reader/Writer Split) — `EventReader` exists
- PRD 16 (Tail/Poll) — `has_new_events` and `active_log_size` exist

## Motivation

PRD 16 provides poll-based tailing: call `has_new_events` in a loop with a sleep interval. This works but introduces latency equal to the sleep interval (50ms–1s typically). For interactive applications (the Leptos todo app, real-time dashboards), sub-millisecond notification is desirable.

`wait_for_events` blocks until the OS reports that `app.jsonl` has been modified, or until a timeout elapses. eventfold-es wraps this in `spawn_blocking` to integrate with async runtimes.

## Scope

**In scope:**
- `WaitResult` enum — `NewData(u64)` or `Timeout`
- `EventReader::wait_for_events(offset, timeout)` — blocking wait using the `notify` crate
- `EventLog::wait_for_events(offset, timeout)` — delegates to reader
- New dependency: `notify` crate (cross-platform file system events)

**Out of scope:**
- Async API (eventfold-es wraps with `spawn_blocking`)
- Subscription management, consumer groups, backpressure
- Rotation-aware tailing (eventfold-es responsibility)

## Types

```rust
/// Result of waiting for new events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitResult {
    /// New data appeared in the active log. Contains the new file size.
    NewData(u64),
    /// The timeout elapsed with no new data.
    Timeout,
}
```

## Implementation

```rust
use notify::{Watcher, RecursiveMode, Event as NotifyEvent, EventKind};
use std::sync::mpsc;
use std::time::Duration;

impl EventReader {
    /// Block until new data appears after `offset` in the active log,
    /// or until `timeout` elapses.
    ///
    /// Uses OS-level file system notifications (inotify on Linux,
    /// kqueue on macOS, ReadDirectoryChangesW on Windows) for
    /// near-zero-latency detection.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use eventfold::{EventReader, WaitResult};
    /// # use std::time::Duration;
    /// let reader = EventReader::new("./data");
    /// let mut offset = 0u64;
    /// loop {
    ///     match reader.wait_for_events(offset, Duration::from_secs(5)).unwrap() {
    ///         WaitResult::NewData(new_size) => {
    ///             for result in reader.read_from(offset).unwrap() {
    ///                 let (event, next_offset, _hash) = result.unwrap();
    ///                 // process event
    ///                 offset = next_offset;
    ///             }
    ///         }
    ///         WaitResult::Timeout => {
    ///             // No new events — do periodic housekeeping, etc.
    ///         }
    ///     }
    /// }
    /// ```
    pub fn wait_for_events(
        &self,
        offset: u64,
        timeout: Duration,
    ) -> io::Result<WaitResult> {
        // Check immediately — data may already be available.
        let current_size = self.active_log_size()?;
        if current_size > offset {
            return Ok(WaitResult::NewData(current_size));
        }

        // Set up a file watcher on the log file.
        let (tx, rx) = mpsc::channel();
        let mut watcher = notify::recommended_watcher(move |res: Result<NotifyEvent, _>| {
            if let Ok(event) = res {
                if matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_)) {
                    let _ = tx.send(());
                }
            }
        })
        .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        watcher
            .watch(self.log_path.parent().unwrap_or(&self.log_path), RecursiveMode::NonRecursive)
            .map_err(|e| io::Error::new(io::ErrorKind::Other, e))?;

        // Re-check after watcher is set up (avoid TOCTOU race).
        let current_size = self.active_log_size()?;
        if current_size > offset {
            return Ok(WaitResult::NewData(current_size));
        }

        // Wait for a notification or timeout.
        match rx.recv_timeout(timeout) {
            Ok(()) => {
                let new_size = self.active_log_size()?;
                if new_size > offset {
                    Ok(WaitResult::NewData(new_size))
                } else {
                    // Spurious wakeup (e.g., metadata change, not a write).
                    // For simplicity, return Timeout. Caller will retry.
                    Ok(WaitResult::Timeout)
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(WaitResult::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(io::Error::new(io::ErrorKind::Other, "file watcher disconnected"))
            }
        }
    }
}
```

### Design Notes

- **Immediate check first.** Before setting up a watcher, check if data is already available. This avoids the overhead of watcher setup when events are already queued.

- **TOCTOU guard.** After setting up the watcher but before blocking on `recv_timeout`, re-check the file size. This closes the race where an event is appended between the initial check and the watcher registration.

- **Watcher scope.** The watcher is created and destroyed per call. This is simple and correct. For high-frequency polling, eventfold-es can hold a persistent watcher — but that's above eventfold's layer.

- **Spurious wakeups.** File system events can fire for metadata changes (permissions, atime updates) that don't add data. The implementation re-checks `active_log_size()` after a notification and returns `Timeout` on spurious wakeups. Callers should be prepared for this.

- **Platform behavior.** The `notify` crate uses inotify (Linux), kqueue (macOS), and ReadDirectoryChangesW (Windows). All are well-tested and production-grade.

- **Feature gate consideration.** If the `notify` dependency is considered too heavy for users who don't need blocking tail, this could be behind a cargo feature flag (e.g., `features = ["blocking-tail"]`). However, `notify` is lightweight and widely used, so defaulting to included is reasonable.

### EventLog Delegation

```rust
impl EventLog {
    pub fn wait_for_events(
        &self,
        offset: u64,
        timeout: Duration,
    ) -> io::Result<WaitResult> {
        self.reader.wait_for_events(offset, timeout)
    }
}
```

## Dependencies

```toml
[dependencies]
notify = "7"
```

`notify` v7 is the current stable release. It has minimal transitive dependencies and is the de facto standard for cross-platform file watching in Rust.

## Files

| File | Action |
|------|--------|
| `Cargo.toml` | Update — add `notify = "7"` |
| `src/log.rs` | Update — add `WaitResult`, `EventReader::wait_for_events`, `EventLog::wait_for_events` |
| `src/lib.rs` | Update — re-export `WaitResult` |
| `tests/blocking_tail_tests.rs` | Create |

## Acceptance Criteria

1. **Returns immediately if data exists:** `wait_for_events(0, 5s)` returns `NewData` immediately when log is non-empty
2. **Blocks and detects new write:** spawn a thread that appends after 100ms, `wait_for_events` returns `NewData` within 500ms
3. **Times out when no write:** `wait_for_events(offset, 200ms)` on quiet log returns `Timeout` after ~200ms
4. **NewData contains correct size:** returned size matches `active_log_size()` at time of detection
5. **No data loss:** after `wait_for_events` returns `NewData`, `read_from(offset)` returns all new events
6. **TOCTOU safe:** append immediately before calling `wait_for_events`, returns `NewData` (not missed)
7. **EventLog delegates:** `EventLog::wait_for_events` works identically
8. **Cargo builds and all tests pass with `cargo clippy -- -D warnings`**

## Test Plan (`tests/blocking_tail_tests.rs`)

- `test_wait_returns_immediately_with_existing_data` — append event, call `wait_for_events(0, 1s)`, returns `NewData` without delay
- `test_wait_detects_new_append` — spawn writer thread that sleeps 100ms then appends, main thread calls `wait_for_events`, returns `NewData`
- `test_wait_timeout` — no writes, `wait_for_events(0, 200ms)` returns `Timeout`, elapsed time is ~200ms
- `test_wait_new_data_size_correct` — returned size in `NewData` matches actual `active_log_size()`
- `test_wait_read_after_detection` — after `NewData`, `read_from(offset)` returns the appended events
- `test_wait_toctou_safety` — append event, immediately call `wait_for_events` with pre-append offset, returns `NewData` (not missed by race)
- `test_wait_multiple_rounds` — simulate a tail loop: wait → read → wait → read, verify all events consumed exactly once
- `test_eventlog_wait_delegates` — `EventLog::wait_for_events` matches reader behavior
