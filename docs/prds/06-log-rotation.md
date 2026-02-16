# PRD 06: Log Rotation & Archival

## Summary

Implement log rotation: compress the active log into a zstd archive and support reading the full event history (archive + active log). The archive is append-only via zstd frame concatenation — each rotation adds a new compressed frame.

## Prerequisites

- PRD 01 (Event type)
- PRD 02 (EventLog core)
- PRD 03 (Snapshot persistence)
- PRD 04 (Views — refresh must work before rotation can happen)

## Scope

**In scope:**
- `EventLog::rotate()` — compress active log, append to archive, truncate, reset snapshot offsets
- `EventLog::read_full()` — stream archive + active log as one continuous event sequence
- Zstd frame concatenation for the archive
- Update `View::refresh` to use `read_full()` when no snapshot exists (full replay through archive)
- Update `View::rebuild` to replay through archive

**Out of scope:**
- Auto-rotation on append (PRD 07 — requires max_log_size config)
- Builder pattern (PRD 07)
- Archive pruning (future, not planned)

## Design

### Rotation Process (`rotate()`)

1. **Refresh all views** — every snapshot now reflects everything in `app.jsonl`. This requires the log to know about its views (passed as parameter or registered).
2. **Read `app.jsonl`** contents into memory
3. **Compress** the contents as a single zstd frame
4. **Append** the compressed frame to `archive.jsonl.zst` (create file if first rotation)
5. **Truncate `app.jsonl`** to zero bytes
6. **Reset all view snapshot offsets to 0** (their state is complete, the events are now in the archive)
7. **Sync** archive and log files

Since views aren't registered with the log yet (PRD 07), `rotate` takes a mutable slice of views:

```rust
impl EventLog {
    pub fn rotate(&mut self, views: &mut [&mut dyn ViewOps]) -> io::Result<()>;
}
```

We need a trait object approach to handle views of different state types:

```rust
pub trait ViewOps {
    fn refresh_boxed(&mut self, log: &EventLog) -> io::Result<()>;
    fn reset_offset(&mut self);
    fn name(&self) -> &str;
}

impl<S: Serialize + DeserializeOwned + Default + Clone> ViewOps for View<S> {
    fn refresh_boxed(&mut self, log: &EventLog) -> io::Result<()> {
        self.refresh(log)?;
        Ok(())
    }
    fn reset_offset(&mut self) { self.offset = 0; self.hash = String::new(); }
    fn name(&self) -> &str { &self.name }
}
```

### `read_full()` — Full History Iterator

```rust
impl EventLog {
    pub fn read_full(&self) -> io::Result<impl Iterator<Item = io::Result<(Event, String)>>>;
}
```

1. If `archive.jsonl.zst` exists:
   - Open and create a zstd streaming decoder
   - Read decompressed output line by line
   - Yield `(event, line_hash)` for each line
2. Then open `app.jsonl` from byte 0:
   - Read line by line
   - Yield `(event, line_hash)` for each line
3. The two streams are chained transparently — the consumer sees one continuous sequence

No byte offset tracking during archive reading — offsets only matter for the active log, and a full replay always ends at the current end of `app.jsonl`.

### View Updates

`View::refresh` changes:
- When no snapshot exists (first refresh or after rebuild), use `log.read_full()` instead of `log.read_from(0)`
- This ensures new views or rebuilds replay the full history including archived events
- The snapshot written after a full replay stores the offset within `app.jsonl` (not the archive) — this is the byte position after the last event in the active log

`View::rebuild` changes:
- Already calls `refresh` after resetting — inherits the `read_full` behavior

### Zstd Frame Concatenation

Zstd natively supports concatenated frames. Each call to `rotate()` produces one frame appended to the archive. On decompression, the zstd decoder streams through all frames transparently as one continuous byte stream.

This means `archive.jsonl.zst` is itself append-only — just like the event log.

## Dependencies

Add to `Cargo.toml`:

```toml
[dependencies]
zstd = "0.13"
```

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Add `rotate()`, `read_full()` |
| `src/view.rs` | Add `ViewOps` trait, update `refresh` for full replay, update `rebuild` |
| `src/archive.rs` | Create — compression/decompression helpers |
| `src/lib.rs` | Update — re-export archive if needed |
| `tests/rotation_tests.rs` | Create |

## Implementation Details

### `archive.rs`

```rust
// src/archive.rs

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write, BufRead, BufReader};
use std::path::Path;

/// Compress data and append as a new zstd frame to the archive file.
/// Creates the archive file if it doesn't exist.
pub fn append_compressed_frame(archive_path: &Path, data: &[u8]) -> io::Result<()>;

/// Open the archive and return a streaming decompressor that reads through
/// all concatenated frames as one continuous byte stream.
/// Returns None if archive doesn't exist.
pub fn open_archive_reader(archive_path: &Path) -> io::Result<Option<impl BufRead>>;
```

### Rotation edge cases

- **Empty active log:** If `app.jsonl` is empty (0 bytes), `rotate()` is a no-op. Don't append an empty frame.
- **No views registered:** Rotation still works — just skip the refresh step. (But warn — snapshots won't be updated.)
- **Archive doesn't exist yet:** First rotation creates it.
- **Crash during rotation:** Handled in PRD 05 and crash safety tests. The key invariant: events may be duplicated between archive and active log after a crash, but never lost.

### read_full() behavior with offset tracking

When `read_full()` transitions from archive to active log, the consumer needs to know the final byte offset in `app.jsonl`. Two approaches:

1. **Return offset from read_full:** Change signature to track position in active log
2. **Simpler:** After `read_full()` finishes, the offset is simply the current size of `app.jsonl`

Go with option 2: after folding all events from `read_full()`, the view sets its offset to the end of `app.jsonl`. This is correct because it has now consumed everything.

## Acceptance Criteria

1. **Basic rotation:** Append events, rotate → `app.jsonl` is empty (0 bytes), `archive.jsonl.zst` exists
2. **Archive contains events:** After rotation, decompress archive → contains all pre-rotation events in order
3. **View offsets reset:** After rotation, all view snapshot offsets are 0
4. **View state unchanged:** After rotation, view state still reflects all events (nothing lost)
5. **Post-rotation appends:** Append after rotation → new events go to fresh `app.jsonl`
6. **Post-rotation refresh:** Append after rotation, refresh view → new events folded in correctly
7. **Multiple rotations:** Rotate 3 times with events between each → archive contains all events from all rotations in order
8. **read_full() all events:** After multiple rotations, `read_full()` yields every event ever appended, in order
9. **New view after rotation:** Create a view after rotation → first refresh replays archive + active log, produces correct state
10. **Empty log rotation:** Rotate with empty `app.jsonl` → no-op, no empty frame appended
11. **Full replay matches incremental:** State from `read_full()` replay matches state from incremental refreshes
12. **Cargo builds and all tests pass**

## Test Plan (`tests/rotation_tests.rs`)

- `test_basic_rotation` — append 10, rotate, verify app.jsonl empty, archive exists
- `test_archive_contains_events` — rotate, decompress, verify event count and order
- `test_view_offsets_reset_after_rotation` — verify offset is 0 in snapshot
- `test_view_state_unchanged_after_rotation` — state before rotation == state after
- `test_post_rotation_appends` — rotate, append 5 more, verify app.jsonl has 5 events
- `test_post_rotation_refresh` — rotate, append, refresh → correct cumulative state
- `test_multiple_rotations` — rotate 3 times, verify archive integrity
- `test_read_full_after_rotations` — multiple rotations, read_full returns all events in order
- `test_new_view_after_rotation` — create view after rotating, refresh → full history replayed
- `test_empty_log_rotation_noop` — rotate empty log, no archive change
- `test_rotation_with_no_views` — rotate without views registered, archive still created
- `test_full_replay_matches_incremental` — compare state from read_full replay vs incremental refreshes
- `test_read_full_no_archive` — no archive exists, read_full returns only active log events
- `test_read_full_empty_everything` — no archive, empty log → empty iterator
