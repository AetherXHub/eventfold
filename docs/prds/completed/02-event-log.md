> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 02: Event Log Core

## Summary

Implement `EventLog` — the append-only log file manager. Handles opening/creating the data directory, appending events to `app.jsonl`, and reading events back with byte offsets and line hashes.

## Prerequisites

- PRD 01 (Event type)

## Scope

**In scope:**
- `EventLog::open(dir)` — create directory structure, open `app.jsonl`
- `EventLog::append(event)` — serialize, append line, flush, return byte offset
- `EventLog::read_from(offset)` — iterator yielding `(Event, next_offset, line_hash)` from active log
- Line hashing with xxhash for integrity checking
- Directory layout: `data/app.jsonl`, `data/views/`
- Note existence of `archive.jsonl.zst` path (used later in PRD 06)

**Out of scope:**
- `read_full()` with archive streaming (PRD 06)
- `rotate()` (PRD 06)
- Auto-rotation / max_log_size (PRD 07)
- Builder pattern (PRD 07)
- View registration (PRD 07)

## Types

```rust
// src/log.rs

use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Seek, SeekFrom, Write};
use std::path::PathBuf;

pub struct EventLog {
    dir: PathBuf,
    log_path: PathBuf,       // app.jsonl
    archive_path: PathBuf,   // archive.jsonl.zst
    file: File,              // handle to app.jsonl, opened in append mode
    views_dir: PathBuf,
}
```

## API

### `EventLog::open(dir: impl AsRef<Path>) -> io::Result<Self>`

- Create `dir` if it doesn't exist
- Create `dir/views/` if it doesn't exist
- Open or create `dir/app.jsonl` in append mode
- Set `archive_path` to `dir/archive.jsonl.zst` (file may not exist yet)
- Return `EventLog`

### `EventLog::append(&mut self, event: &Event) -> io::Result<u64>`

- Get current file length (this is the byte offset of the new event)
- Serialize event to JSON string (single line)
- Write `json + "\n"` to `app.jsonl`
- Flush the file
- Return the byte offset where this event starts

### `EventLog::read_from(&self, offset: u64) -> io::Result<impl Iterator<Item = io::Result<(Event, u64, String)>>>`

- Open `app.jsonl` for reading (separate file handle)
- Seek to `offset`
- Return iterator that for each line:
  - Computes xxhash of the raw line bytes (before parsing)
  - Parses JSON into `Event`
  - Yields `(event, next_byte_offset, hex_hash)`
  - Skips empty lines
  - Returns error for malformed JSON lines

### Hashing

```rust
// Line hash: xxh64 of the raw line bytes (without trailing newline), hex-encoded
fn line_hash(line: &[u8]) -> String {
    let hash = xxhash_rust::xxh64::xxh64(line, 0);
    format!("{:016x}", hash)
}
```

## Dependencies

Add to `Cargo.toml`:

```toml
[dependencies]
xxhash-rust = { version = "0.8", features = ["xxh64"] }
```

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Create |
| `src/lib.rs` | Update — re-export `EventLog` |
| `Cargo.toml` | Add xxhash-rust |
| `tests/common/mod.rs` | Add `append_n()` helper |
| `tests/log_tests.rs` | Create |

## Implementation Details

- The append file handle is kept open for the lifetime of `EventLog`. Appends write to this handle.
- Reading uses a **separate** file handle opened read-only each time `read_from` is called. This avoids seeking the append handle.
- `flush()` after every append ensures durability. Use `file.sync_data()` for fsync-level guarantee.
- Byte offsets are positions in the file, not event indices. This allows O(1) seeking.
- The iterator must track its position as bytes read, not lines counted.
- Partial lines (no trailing newline — possible after a crash) should be skipped silently. Log a warning if desired but do not error.

## Acceptance Criteria

1. **Open creates directory structure:** `EventLog::open("./test_data")` creates the directory, `views/` subdirectory, and `app.jsonl` file
2. **Append single event:** Append an event, read it back with `read_from(0)`, verify it matches
3. **Append multiple events:** Append N events, read all back in order, verify sequence
4. **Byte offsets correct:** The offset returned by `append` can be used with `read_from` to seek directly to that event
5. **Offset chaining:** `read_from(0)` yields `(event, next_offset, hash)` — using `next_offset` with `read_from` skips past that event
6. **Empty log:** `read_from(0)` on empty log returns empty iterator
7. **Hash determinism:** Same event bytes always produce the same hash
8. **Reopen persistence:** Close `EventLog`, reopen same directory — previous events are still readable, new appends go after existing data
9. **Special characters:** Events with unicode, newlines in string values, escaped quotes survive the append/read cycle
10. **Partial line handling:** A log file with a partial last line (no trailing newline) — `read_from` skips it without error
11. **Cargo builds and all tests pass**

## Test Plan

### Test Helpers Addition (`tests/common/mod.rs`)

```rust
use eventfold::EventLog;

pub fn append_n(log: &mut EventLog, n: usize) {
    for i in 0..n {
        let event = dummy_event(&format!("event_{}", i));
        log.append(&event).unwrap();
    }
}
```

### Test Cases (`tests/log_tests.rs`)

- `test_open_creates_directory` — open with nonexistent path, verify dir + views/ + app.jsonl exist
- `test_open_existing_directory` — open twice, no error
- `test_append_single_event` — append one, read back, verify
- `test_append_multiple_events` — append 10, read all, verify order
- `test_read_from_zero` — returns all events
- `test_read_from_offset` — append 5 events, capture offset of 3rd, read_from that offset, verify only events 3-4 returned
- `test_byte_offset_correctness` — append events, verify each returned offset can seek to the right event
- `test_empty_log` — read_from(0) on fresh log returns nothing
- `test_hash_determinism` — append same event content twice, hashes of identical lines match
- `test_reopen_persistence` — open, append 3, drop, reopen, verify 3 events readable, append 2 more, verify 5 total
- `test_special_characters` — unicode and escaped content round-trip
- `test_partial_line_skipped` — manually write a partial line (no newline) to app.jsonl, verify read_from skips it
- `test_read_from_end_of_file` — read_from with offset at EOF returns empty iterator
