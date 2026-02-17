# PRD 14: Conditional Append

## Summary

Add `append_if()` to `EventWriter` — an optimistic concurrency primitive that appends an event only if the log's current state (offset and hash) matches the caller's expectations. If another write has occurred since the caller last read, the append is rejected with a conflict error instead of silently creating a race.

## Prerequisites

- PRD 12 (Reader/Writer Split) — `EventWriter` exists
- PRD 13 (AppendResult) — `append` returns offset + hash for tracking

## Motivation

eventfold-es needs optimistic concurrency per stream: "append this event only if no one else has written since I last read." Business invariants (e.g., "an order can only be placed if the account has sufficient balance") depend on decisions being made against a known state. Without conditional append, two concurrent writers could both read the same state, make conflicting decisions, and both succeed.

The check must happen at the file level, not in a layer above, because only the writer holds the exclusive lock and can atomically verify-then-write.

## Scope

**In scope:**
- `AppendConflict` error type — carries expected vs actual offset/hash
- `ConditionalAppendError` enum — wraps both conflicts and I/O errors
- `EventWriter::append_if(event, expected_offset, expected_hash)` — verify-then-append
- Convenience: `EventLog::append_if()` delegating to writer

**Out of scope:**
- Retry logic (caller's responsibility)
- Transaction / multi-event atomicity
- File locking (PRD 15 — but conditional append works within a single writer regardless)

## Types

```rust
/// Conflict details when a conditional append fails.
#[derive(Debug, Clone, PartialEq)]
pub struct AppendConflict {
    /// The offset the caller expected the log to be at.
    pub expected_offset: u64,
    /// The actual current offset (file size).
    pub actual_offset: u64,
    /// The hash the caller expected.
    pub expected_hash: String,
    /// The actual hash of the last line, if the offset matched
    /// but the hash didn't. `None` if the offset check failed first.
    pub actual_hash: Option<String>,
}

/// Error type for conditional append operations.
#[derive(Debug)]
pub enum ConditionalAppendError {
    /// The log state didn't match expectations — no write occurred.
    Conflict(AppendConflict),
    /// An I/O error occurred during the check or write.
    Io(io::Error),
}
```

`ConditionalAppendError` implements `std::fmt::Display`, `std::error::Error`, and `From<io::Error>`.

## Implementation

```rust
impl EventWriter {
    /// Append an event only if the log's current state matches expectations.
    ///
    /// Checks that the active log's file size equals `expected_offset` and
    /// (if non-zero) that the hash of the last event line matches
    /// `expected_hash`. If either check fails, returns
    /// `Err(ConditionalAppendError::Conflict(...))` without writing.
    ///
    /// For an empty log, pass `expected_offset: 0` and `expected_hash: ""`.
    ///
    /// On success, returns the same `AppendResult` as `append()`.
    pub fn append_if(
        &mut self,
        event: &Event,
        expected_offset: u64,
        expected_hash: &str,
    ) -> Result<AppendResult, ConditionalAppendError> {
        let current_size = self.active_log_size()?;

        // Fast path: offset mismatch means someone else wrote.
        if current_size != expected_offset {
            return Err(ConditionalAppendError::Conflict(AppendConflict {
                expected_offset,
                actual_offset: current_size,
                expected_hash: expected_hash.to_string(),
                actual_hash: None,
            }));
        }

        // If log is non-empty, verify the last line hash.
        if expected_offset > 0 {
            let reader = self.reader();
            let actual_hash = reader
                .read_line_hash_before(expected_offset)?
                .unwrap_or_default();
            if actual_hash != expected_hash {
                return Err(ConditionalAppendError::Conflict(AppendConflict {
                    expected_offset,
                    actual_offset: current_size,
                    expected_hash: expected_hash.to_string(),
                    actual_hash: Some(actual_hash),
                }));
            }
        }

        // Checks passed — proceed with normal append.
        Ok(self.append(event)?)
    }
}
```

### EventLog Delegation

```rust
impl EventLog {
    /// Conditional append — delegates to the inner writer.
    pub fn append_if(
        &mut self,
        event: &Event,
        expected_offset: u64,
        expected_hash: &str,
    ) -> Result<AppendResult, ConditionalAppendError> {
        let result = self.writer.append_if(event, expected_offset, expected_hash)?;
        // Auto-rotation check (same as regular append)
        if self.writer.max_log_size > 0
            && self.writer.active_log_size()? >= self.writer.max_log_size
        {
            self.rotate()?;
        }
        Ok(result)
    }
}
```

### Design Notes

- **Offset check is the fast path.** Just compares file size — no disk read. The hash check only runs when offsets match, which is the common non-conflict case.
- **Empty log convention.** `expected_offset: 0, expected_hash: ""` means "I expect the log to be empty." The hash check is skipped when `expected_offset == 0`.
- **No retry.** The caller (eventfold-es) is responsible for re-reading state and retrying. The conflict error provides enough information to decide whether to retry.
- **Single-writer safety.** Even though `EventWriter` is exclusive, conditional append is still valuable — the writer may be shared via `Arc<Mutex<>>` with multiple logical actors (as in the Leptos example).

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Update — add `AppendConflict`, `ConditionalAppendError`, `EventWriter::append_if`, `EventLog::append_if` |
| `src/lib.rs` | Update — re-export `AppendConflict`, `ConditionalAppendError` |
| `tests/conditional_append_tests.rs` | Create |

## Acceptance Criteria

1. **Succeeds when state matches:** `append_if` with correct offset and hash appends the event and returns `AppendResult`
2. **Fails on offset mismatch:** returns `Conflict` with correct expected/actual offsets, `actual_hash` is `None`
3. **Fails on hash mismatch:** offset matches but hash differs, returns `Conflict` with `actual_hash` populated
4. **Empty log convention:** `append_if(event, 0, "")` succeeds on empty log
5. **No write on conflict:** file size unchanged after a conflict error
6. **Chainable:** successful `append_if` returns `AppendResult` that can be used as the next expected state
7. **AppendResult matches append:** the `AppendResult` from `append_if` is identical to what `append` would have returned
8. **EventLog delegates:** `EventLog::append_if` works and handles auto-rotation
9. **Error types implement Display and Error traits**
10. **Cargo builds and all tests pass with `cargo clippy -- -D warnings`**

## Test Plan (`tests/conditional_append_tests.rs`)

- `test_append_if_empty_log` — `append_if(event, 0, "")` succeeds on fresh log
- `test_append_if_matches` — append one event, use its `AppendResult` as expected state, `append_if` succeeds
- `test_append_if_chain` — append three events conditionally in sequence, each using the previous result
- `test_append_if_offset_mismatch` — append an event, then `append_if` with `expected_offset: 0` fails with offset conflict
- `test_append_if_hash_mismatch` — construct a state with correct offset but wrong hash, fails with hash conflict
- `test_append_if_no_write_on_conflict` — after conflict, file size unchanged, `read_from(0)` returns only the original events
- `test_append_if_result_matches_append` — compare `AppendResult` fields from `append_if` vs `append` for equivalent scenarios
- `test_append_if_concurrent_simulation` — two "actors" both read state, first `append_if` succeeds, second fails with conflict
- `test_eventlog_append_if_delegates` — `EventLog::append_if` works identically to direct writer call
- `test_eventlog_append_if_auto_rotation` — `EventLog::append_if` with small `max_log_size` triggers rotation after conditional append
