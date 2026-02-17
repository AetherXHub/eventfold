# PRD 13: AppendResult with Offset and Hash

## Summary

Change `append()` to return an `AppendResult` struct containing the start offset, end offset, and line hash of the written event. This gives callers the information they need for version tracking and conditional appends without a second read pass.

## Prerequisites

- PRD 12 (Reader/Writer Split) — `append` lives on `EventWriter`

## Motivation

`append()` currently returns `io::Result<u64>` — just the byte offset where the event starts. eventfold-es needs to track the stream's version after each write: the offset *after* the trailing newline (where the next event would begin) and the hash of the written line. These are the exact values checked during conditional appends (PRD 14).

The line hash computation already exists (`line_hash()`) but isn't used on the write path. Computing it during append is essentially free — the serialized JSON bytes are already in memory.

## Scope

**In scope:**
- New `AppendResult` struct with `start_offset`, `end_offset`, `line_hash`
- `EventWriter::append` returns `io::Result<AppendResult>` instead of `io::Result<u64>`
- `EventLog::append` returns `io::Result<AppendResult>` (breaking change to return type)
- Update all callers

**Out of scope:**
- Conditional append (PRD 14)
- Any changes to read paths

## Types

```rust
/// Result of a successful append operation.
#[derive(Debug, Clone, PartialEq)]
pub struct AppendResult {
    /// Byte offset where the event line starts in `app.jsonl`.
    pub start_offset: u64,

    /// Byte offset after the trailing newline — the position where
    /// the next event would begin.
    pub end_offset: u64,

    /// xxh64 hash of the serialized event line (hex-encoded, without
    /// the trailing newline).
    pub line_hash: String,
}
```

## Implementation Details

### Write Path Change

Current:
```rust
pub fn append(&mut self, event: &Event) -> io::Result<u64> {
    let offset = self.file.seek(SeekFrom::End(0))?;
    let json = serde_json::to_string(event)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    writeln!(self.file, "{json}")?;
    self.file.sync_data()?;
    // ... auto-rotation check ...
    Ok(offset)
}
```

After:
```rust
pub fn append(&mut self, event: &Event) -> io::Result<AppendResult> {
    let start_offset = self.file.seek(SeekFrom::End(0))?;
    let json = serde_json::to_string(event)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
    let hash = line_hash(json.as_bytes());
    writeln!(self.file, "{json}")?;
    self.file.sync_data()?;
    let end_offset = start_offset + json.len() as u64 + 1; // +1 for '\n'
    // ... auto-rotation check ...
    Ok(AppendResult {
        start_offset,
        end_offset,
        line_hash: hash,
    })
}
```

The `end_offset` is computed arithmetically rather than a second seek — the serialized JSON length plus 1 for the newline.

### Caller Updates

Most callers only use the start offset (or ignore the return entirely). The migration is mechanical:

```rust
// Before
let offset = log.append(&event)?;

// After
let result = log.append(&event)?;
let offset = result.start_offset; // if only start offset is needed
```

Or simply `log.append(&event)?;` if the return is unused.

### Hash Consistency

The hash computed on the write path (`line_hash(json.as_bytes())`) must be identical to what the read path computes. The read path hashes the raw line bytes without the trailing newline. The write path hashes `json.as_bytes()` — which is the same bytes, before the newline is added by `writeln!`. These match.

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Update — define `AppendResult`, change `append` return type |
| `src/lib.rs` | Update — re-export `AppendResult` |
| `tests/log_tests.rs` | Update — adapt to `AppendResult` return, add new assertions |
| `tests/builder_tests.rs` | Update — adapt append calls |
| `tests/rotation_tests.rs` | Update — adapt append calls |
| `tests/view_tests.rs` | Update — adapt append calls |
| `tests/integrity_tests.rs` | Update — adapt append calls |
| `tests/crash_safety.rs` | Update — adapt append calls |
| `tests/props.rs` | Update — adapt append calls |
| `tests/common/mod.rs` | Update — `append_n` helper uses new return type |
| `examples/*.rs` | Update — adapt append calls |
| `examples-leptos/todo-app/src/server.rs` | Update — adapt append calls |

## Acceptance Criteria

1. **AppendResult fields correct:** `start_offset` matches pre-write file position, `end_offset` = `start_offset` + JSON length + 1, `line_hash` matches `line_hash()` of the serialized bytes
2. **Hash matches read path:** hash from `AppendResult` equals hash from `reader.read_from(start_offset)`
3. **End offset is next start:** appending two events, the second's `start_offset` equals the first's `end_offset`
4. **Empty log starts at 0:** first append to empty log has `start_offset == 0`
5. **EventLog delegates correctly:** `EventLog::append` returns `AppendResult` from inner writer
6. **All existing tests pass** after updating to new return type
7. **Cargo builds and all tests pass with `cargo clippy -- -D warnings`**

## Test Plan

### Updated Tests

All existing tests that call `append` must be updated to handle `AppendResult`. Most only need `.start_offset` or can ignore the return.

### New Test Cases (`tests/log_tests.rs`)

- `test_append_result_start_offset` — first append to empty log has `start_offset == 0`
- `test_append_result_end_offset` — `end_offset` equals `start_offset` + serialized JSON length + 1
- `test_append_result_consecutive` — second append's `start_offset` equals first's `end_offset`
- `test_append_result_hash_matches_read` — hash from `AppendResult` matches hash from `read_from`
- `test_append_result_hash_deterministic` — same event appended twice produces same hash
