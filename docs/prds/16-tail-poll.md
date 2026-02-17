# PRD 16: Tail / New-Event Detection

## Summary

Add lightweight polling primitives to `EventReader` for detecting new events — `has_new_events(offset)` and `active_log_size()`. These are the building blocks eventfold-es needs for subscriptions without adding async or platform-specific file watchers to eventfold itself.

## Prerequisites

- PRD 12 (Reader/Writer Split) — `EventReader` exists

## Motivation

eventfold-es needs subscriptions. Subscriptions need tailing: "notify me when new events appear after my current position." The lowest-level question — "are there new events after offset N?" — belongs in eventfold because it's about a single file.

These primitives are intentionally minimal. eventfold stays synchronous. eventfold-es wraps them in async timers or `spawn_blocking`. A blocking `wait_for_events` with inotify/kqueue could come later as an optimization, but poll-based tailing works fine initially and avoids adding platform-specific dependencies.

## Scope

**In scope:**
- `EventReader::has_new_events(offset)` — non-blocking check, returns `bool`
- `EventReader::active_log_size()` — returns current file size (already exists on `EventLog`, move to reader)
- `EventLog::has_new_events(offset)` — delegates to reader

**Out of scope:**
- Blocking wait (`wait_for_events`) — deferred to a future PRD
- inotify / kqueue / platform-specific file watchers
- Async API
- Subscription management (eventfold-es concern)

## Implementation

```rust
impl EventReader {
    /// Returns `true` if the active log contains data beyond `offset`.
    ///
    /// This is a non-blocking metadata check (stat call). Use it to
    /// implement poll-based tailing:
    ///
    /// ```no_run
    /// # use eventfold::EventReader;
    /// let reader = EventReader::new("./data");
    /// let mut offset = 0u64;
    /// loop {
    ///     if reader.has_new_events(offset).unwrap() {
    ///         for result in reader.read_from(offset).unwrap() {
    ///             let (event, next_offset, _hash) = result.unwrap();
    ///             // process event
    ///             offset = next_offset;
    ///         }
    ///     }
    ///     std::thread::sleep(std::time::Duration::from_millis(50));
    /// }
    /// ```
    pub fn has_new_events(&self, offset: u64) -> io::Result<bool> {
        Ok(fs::metadata(&self.log_path)?.len() > offset)
    }

    /// Returns the current size of `app.jsonl` in bytes.
    ///
    /// This is a lightweight "version" check — if the size hasn't
    /// changed, no new events have been appended.
    pub fn active_log_size(&self) -> io::Result<u64> {
        Ok(fs::metadata(&self.log_path)?.len())
    }
}
```

Both methods are pure metadata calls — no file opens, no reads, just `stat`. They're as cheap as a filesystem operation can be.

### EventLog Delegation

```rust
impl EventLog {
    /// Returns `true` if there are events beyond `offset` in the active log.
    pub fn has_new_events(&self, offset: u64) -> io::Result<bool> {
        self.reader.has_new_events(offset)
    }
}
```

`active_log_size()` already exists on `EventLog` — it should delegate to `self.reader.active_log_size()` after the reader/writer split (PRD 12 handles this).

### Design Notes

- **No false positives from rotation.** After rotation, `app.jsonl` is truncated to 0 bytes. A subscriber holding `offset = 500` would see `has_new_events(500)` return `false` (size 0 < 500). This is correct — the subscriber's offset is now stale relative to the active log. Handling rotation-aware tailing (detecting that a rotation occurred and re-reading from the archive) is eventfold-es's responsibility.

- **Rotation detection hint.** Callers can detect rotation by checking `active_log_size() < remembered_offset`. This doesn't require a new API — it falls out naturally.

- **Thread safety.** `EventReader` is `Clone + Send + Sync` (PathBuf fields only). Multiple threads can call `has_new_events` concurrently without issues.

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Update — add `has_new_events` to `EventReader` and `EventLog` |
| `src/lib.rs` | No change (methods are on already-exported types) |
| `tests/tail_tests.rs` | Create |

## Acceptance Criteria

1. **Empty log returns false:** `has_new_events(0)` on empty log returns `false`
2. **Returns true after append:** append an event, `has_new_events(0)` returns `true`
3. **Returns false at current position:** append an event, get `end_offset` from `AppendResult`, `has_new_events(end_offset)` returns `false`
4. **Returns true after second append:** two appends, `has_new_events(first_end_offset)` returns `true`
5. **active_log_size matches:** `active_log_size()` equals `AppendResult::end_offset` after each append
6. **Returns false after rotation:** append, rotate, `has_new_events(end_offset)` returns `false` (log is now empty)
7. **Metadata only:** `has_new_events` does not open the file or read any content (verified by behavior, not implementation detail)
8. **EventLog delegates:** `EventLog::has_new_events` matches `EventReader::has_new_events`
9. **Cargo builds and all tests pass with `cargo clippy -- -D warnings`**

## Test Plan (`tests/tail_tests.rs`)

- `test_has_new_events_empty_log` — fresh log, `has_new_events(0)` returns `false`
- `test_has_new_events_after_append` — append one event, `has_new_events(0)` returns `true`
- `test_has_new_events_at_current_offset` — append, use `end_offset`, `has_new_events(end_offset)` returns `false`
- `test_has_new_events_after_multiple_appends` — append two events, check at various offsets
- `test_has_new_events_after_rotation` — append, rotate, check with pre-rotation offset returns `false`
- `test_active_log_size_empty` — fresh log, `active_log_size()` returns 0
- `test_active_log_size_after_append` — matches `AppendResult::end_offset`
- `test_active_log_size_after_rotation` — returns 0 after rotation
- `test_poll_loop_simulation` — append events in sequence, simulate a poll loop that catches up, verify all events seen exactly once
- `test_eventlog_has_new_events` — `EventLog::has_new_events` delegates correctly
