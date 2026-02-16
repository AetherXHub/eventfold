> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 03: Snapshot Persistence

## Summary

Implement snapshot save/load/delete operations. Snapshots are the caching mechanism that makes incremental view refresh possible — they store `(state, byte_offset, hash)` and are written atomically to survive crashes.

## Prerequisites

- PRD 01 (Event type — for serde traits)

## Scope

**In scope:**
- `Snapshot<S>` generic struct
- `snapshot::save(path, snapshot)` — atomic write via `.tmp` + rename
- `snapshot::load(path)` — returns `Option<Snapshot<S>>`
- `snapshot::delete(path)` — removes snapshot file

**Out of scope:**
- Hash verification logic (PRD 05)
- View integration (PRD 04)
- Deciding when to snapshot (PRD 04 — always on refresh if events were processed)

## Types

```rust
// src/snapshot.rs

use serde::{Serialize, Deserialize, de::DeserializeOwned};
use std::path::Path;
use std::io;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot<S> {
    pub state: S,
    pub offset: u64,
    pub hash: String,
}
```

## API

### `save<S: Serialize>(path: &Path, snapshot: &Snapshot<S>) -> io::Result<()>`

- Serialize snapshot to JSON (pretty-printed is fine for snapshots — they're human-inspectable)
- Write to `path.with_extension("tmp")` (e.g., `todos.snapshot.json.tmp`)
- `sync_data()` the temp file
- Rename temp file to final path (atomic on POSIX)
- This guarantees: if the process crashes mid-write, the old snapshot survives intact

### `load<S: DeserializeOwned>(path: &Path) -> io::Result<Option<Snapshot<S>>>`

- If file doesn't exist, return `Ok(None)`
- Read file contents
- Deserialize JSON into `Snapshot<S>`
- Return `Ok(Some(snapshot))`
- If deserialization fails (corrupt file), return `Ok(None)` — treat as missing, will trigger rebuild

### `delete(path: &Path) -> io::Result<()>`

- Remove the file at `path`
- If file doesn't exist, return `Ok(())` (idempotent)
- Also remove `.tmp` file if it exists (cleanup)

## Files

| File | Action |
|------|--------|
| `src/snapshot.rs` | Create |
| `src/lib.rs` | Update — re-export `Snapshot` and snapshot functions |
| `tests/snapshot_tests.rs` | Create |

## Implementation Details

- The atomic write pattern (write tmp → sync → rename) is critical for crash safety. Without it, a crash mid-write could leave a half-written snapshot that fails to deserialize.
- Snapshots are pretty-printed JSON for debuggability. Users can `cat views/todos.snapshot.json | jq .` to inspect state.
- The `load` function treats any deserialization error as "no snapshot" rather than a hard error. This triggers a full rebuild, which is always safe.
- The `.tmp` file uses the same directory as the final file to ensure the rename is on the same filesystem (required for atomic rename).

## Acceptance Criteria

1. **Round-trip:** Save a snapshot, load it back — state, offset, and hash are identical
2. **Load nonexistent:** Load from a path that doesn't exist returns `None`
3. **Atomic write:** After save, the `.tmp` file does not exist (was renamed)
4. **Delete removes file:** Save, then delete — load returns `None`
5. **Delete idempotent:** Delete on nonexistent path does not error
6. **Various state types:** Save/load works with: empty struct, struct with Vec, nested structs, large state
7. **Offset zero:** Snapshot with `offset: 0` (fresh after rotation) round-trips correctly
8. **Large offset:** Snapshot with large offset value round-trips correctly
9. **Corrupt file recovery:** Write garbage to snapshot path, load returns `None` (not an error)
10. **Tmp cleanup:** If a `.tmp` file exists (from a previous crash), `delete` removes it too
11. **Cargo builds and all tests pass**

## Test Plan

### Test Cases (`tests/snapshot_tests.rs`)

```rust
#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct TestState {
    count: u64,
    items: Vec<String>,
}
```

- `test_save_load_round_trip` — save with TestState, load, assert equal
- `test_load_nonexistent` — load from `/tmp/does_not_exist.json` → None
- `test_no_tmp_file_after_save` — save, assert `.tmp` path does not exist
- `test_delete_removes_file` — save, delete, load → None
- `test_delete_idempotent` — delete nonexistent → Ok(())
- `test_empty_state` — save/load with `#[derive(Default)] struct Empty {}`
- `test_nested_state` — save/load with nested structs
- `test_large_state` — save/load with state containing 1000 items in a Vec
- `test_offset_zero` — snapshot with offset 0
- `test_large_offset` — snapshot with offset u64::MAX / 2
- `test_corrupt_file_returns_none` — write `"garbage{{{" `to file, load → None
- `test_truncated_file_returns_none` — write partial JSON, load → None
- `test_tmp_cleanup_on_delete` — manually create `.tmp` file, call delete, both gone
