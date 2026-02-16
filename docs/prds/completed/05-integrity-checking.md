> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 05: Integrity Checking

## Summary

Add hash-based integrity verification to detect snapshot/log inconsistencies. When a snapshot's hash doesn't match the log, or its offset is beyond EOF, automatically trigger a full rebuild. This catches corruption, manual edits, and crash-related inconsistencies.

## Prerequisites

- PRD 02 (EventLog — read_from, line hashing)
- PRD 03 (Snapshot — offset and hash fields)
- PRD 04 (View — refresh, rebuild)

## Scope

**In scope:**
- Verify snapshot hash against the actual log content on snapshot load
- Detect offset-beyond-EOF (log was truncated or rotated)
- Auto-rebuild on any integrity mismatch
- Hash verification function in snapshot module

**Out of scope:**
- Checksumming the entire log file
- Detecting inserted/deleted lines in the middle of the log (would require full replay)
- Archive integrity (PRD 06)

## Design

### Verification Strategy

The hash stored in a snapshot is the xxhash of the **last event line processed**. On load, we verify:

1. **Offset within bounds:** `snapshot.offset <= app.jsonl file size`. If not, the log was truncated → rebuild.
2. **Hash verification (when offset > 0):** Read the line immediately *before* `snapshot.offset` (i.e., the last line the snapshot consumed). Compute its hash. Compare with `snapshot.hash`. If mismatch → rebuild.

Reading "the line before offset" means: seek backwards from `offset` to find the previous newline, then read that line. This is the line whose hash should match.

### Simpler Alternative (Recommended)

Rather than seeking backwards (tricky with variable-length lines), use this approach:

1. If `snapshot.offset > file_size` → rebuild
2. If `snapshot.offset > 0`, store the hash as verification but **don't actively verify on load**. Instead, rely on the fact that if the log was tampered with at positions before our offset, the events we care about (after our offset) are still valid. The hash serves as a **forensic marker** — it lets you detect if something changed, but the system self-heals by rebuilding when things don't make sense.
3. Add a `View::verify(&self, log: &EventLog) -> io::Result<bool>` method that performs the explicit check. Users can call this if they want verification. Refresh calls it internally before using a snapshot.

### Recommended Approach

Integrate verification into `View::refresh`:

```rust
// During refresh, after loading snapshot:
fn verify_snapshot(&self, log: &EventLog) -> io::Result<SnapshotValidity> {
    let file_size = log.active_log_size()?;

    if self.offset > file_size {
        return Ok(SnapshotValidity::OffsetBeyondEof);
    }

    if self.offset == 0 {
        return Ok(SnapshotValidity::Valid); // nothing to verify
    }

    // Read the line ending at self.offset and check its hash
    match log.read_line_hash_before(self.offset)? {
        Some(hash) if hash == self.hash => Ok(SnapshotValidity::Valid),
        Some(_) => Ok(SnapshotValidity::HashMismatch),
        None => Ok(SnapshotValidity::Valid), // can't verify, trust it
    }
}

enum SnapshotValidity {
    Valid,
    OffsetBeyondEof,
    HashMismatch,
}
```

When invalid, `refresh` resets to defaults and replays from offset 0 (a rebuild).

## API Changes

### EventLog additions

```rust
impl EventLog {
    /// Returns the current size in bytes of app.jsonl
    pub fn active_log_size(&self) -> io::Result<u64>;

    /// Read the line immediately before the given byte offset and return its hash.
    /// Returns None if offset is 0 or the line can't be read.
    pub fn read_line_hash_before(&self, offset: u64) -> io::Result<Option<String>>;
}
```

### View changes

`View::refresh` now includes integrity verification between snapshot load and event reading. If verification fails, it logs a warning and rebuilds from scratch.

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Add `active_log_size()`, `read_line_hash_before()` |
| `src/view.rs` | Add verification logic to `refresh` |
| `tests/integrity_tests.rs` | Create |

## Implementation Details

### `read_line_hash_before(offset)`

1. Open `app.jsonl` for reading
2. If offset is 0, return `None`
3. Seek to `offset - 1` (this should be the `\n` at the end of the previous line)
4. Scan backwards to find the start of that line (previous `\n` or start of file)
5. Read the line bytes
6. Compute and return xxhash

Alternatively, a simpler approach: read from offset 0 up to `offset`, keeping track of the last complete line and its hash. This is O(offset) but simple and correct. For the target use case (small logs), this is fine.

**Simplest approach (recommended):** Since we track the hash of every line during `read_from`, we can simply re-read the line that ends at `offset`. The offset points to the byte *after* the newline of the last consumed line. So the previous line starts at some point before `offset - 1` and ends at `offset - 1` (the newline). We scan backwards from `offset - 2` to find the start.

### Warning on rebuild

When integrity check fails, emit a warning. Use `eprintln!` for now (no logging framework dependency). The warning should include:
- Which view detected the issue
- What the mismatch was (offset beyond EOF vs hash mismatch)
- That a full rebuild is being triggered

## Acceptance Criteria

1. **Valid snapshot accepted:** Normal operation — snapshot loads, hash matches, offset valid → no rebuild triggered
2. **Offset beyond EOF:** Set snapshot offset to a value larger than `app.jsonl` → triggers rebuild, produces correct state
3. **Hash mismatch:** Manually modify a line in `app.jsonl` (at the position before snapshot offset) → hash mismatch detected → triggers rebuild
4. **Empty log with nonzero offset:** Truncate `app.jsonl` to empty, snapshot has offset > 0 → triggers rebuild
5. **Offset zero always valid:** Snapshot with offset 0 always passes verification (nothing to check)
6. **Rebuild produces correct state:** After any integrity-triggered rebuild, the resulting state matches a fresh full replay
7. **Warning emitted:** When rebuild is triggered due to integrity failure, a warning message is produced
8. **Normal refresh unaffected:** Integrity checking does not change behavior when everything is consistent
9. **Cargo builds and all tests pass**

## Test Plan (`tests/integrity_tests.rs`)

- `test_valid_snapshot_accepted` — normal append/refresh cycle, no rebuild triggered
- `test_offset_beyond_eof` — append 5, refresh, manually truncate app.jsonl to 10 bytes, refresh → rebuild, correct state from remaining events
- `test_hash_mismatch` — append 5, refresh, manually overwrite a line before the snapshot offset, refresh → rebuild
- `test_empty_log_nonzero_offset` — append 5, refresh, truncate app.jsonl to empty, refresh → rebuild from empty (state = default)
- `test_offset_zero_always_valid` — snapshot with offset 0, any log state → accepted
- `test_rebuild_correctness_after_integrity_failure` — trigger rebuild via corruption, verify state matches fresh replay
- `test_manual_log_edit_detected` — insert extra line in middle of app.jsonl, refresh → hash mismatch detected
- `test_no_false_positives` — many append/refresh cycles with no corruption → no unexpected rebuilds
