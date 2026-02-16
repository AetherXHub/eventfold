# PRD 04: Views & Reducers

## Summary

Implement `View<S>` — the primary interface for reading derived state. A view owns a reducer function, manages its snapshot, and supports incremental refresh from the active log. This PRD connects events, the log, and snapshots into a usable read path.

## Prerequisites

- PRD 01 (Event type)
- PRD 02 (EventLog — `read_from`)
- PRD 03 (Snapshot persistence)

## Scope

**In scope:**
- `ReduceFn<S>` type alias
- `View<S>` struct with `new`, `refresh`, `state`, `rebuild`
- Incremental refresh: load snapshot → read new events from active log → fold → save snapshot
- Full replay: no snapshot exists → `read_from(0)` on active log → fold → save snapshot
- Snapshot-as-side-effect: write snapshot if and only if new events were processed

**Out of scope:**
- Reading from archive (`read_full`) — that requires PRD 06. For now, a view with no snapshot replays from `app.jsonl` byte 0 only.
- View registration with EventLog (PRD 07)
- Auto-rotation triggers (PRD 07)
- Hash verification / integrity checking (PRD 05)

## Types

```rust
// src/view.rs

use crate::event::Event;
use crate::log::EventLog;
use crate::snapshot::{self, Snapshot};
use serde::{Serialize, de::DeserializeOwned};
use std::io;
use std::path::PathBuf;

pub type ReduceFn<S> = fn(S, &Event) -> S;

pub struct View<S> {
    name: String,
    reducer: ReduceFn<S>,
    snapshot_path: PathBuf,
    state: S,
    offset: u64,
    hash: String,
    loaded: bool, // whether we've attempted to load from disk
}
```

## API

### `View::new(name: &str, reducer: ReduceFn<S>, views_dir: &Path) -> Self`

- Set `snapshot_path` to `views_dir/{name}.snapshot.json`
- Initialize `state` to `S::default()`
- Set `offset` to 0, `hash` to empty string
- Set `loaded` to false

### `View::refresh(&mut self, log: &EventLog) -> io::Result<&S>`

1. If not `loaded`, attempt to load snapshot from disk:
   - If found: set `state`, `offset`, `hash` from snapshot
   - If not found (or corrupt): keep defaults (offset 0, default state)
   - Mark `loaded = true`
2. Call `log.read_from(self.offset)` to get iterator of new events
3. Fold each event through `self.reducer`, updating state
4. Track the last `next_offset` and `line_hash` seen
5. If any events were processed:
   - Update `self.offset` and `self.hash`
   - Save snapshot to disk
6. If no events were processed: return current state, no disk write
7. Return `&self.state`

### `View::state(&self) -> &S`

- Return reference to current in-memory state
- No I/O, no refresh — just return what we have
- If `refresh` hasn't been called, returns `S::default()`

### `View::rebuild(&mut self, log: &EventLog) -> io::Result<&S>`

- Delete snapshot from disk
- Reset `state` to `S::default()`
- Reset `offset` to 0, `hash` to empty string
- Set `loaded` to true (we know there's no snapshot)
- Call `self.refresh(log)` — replays full active log

## Implementation Details

- The `loaded` flag prevents hitting disk on every `refresh` call. Once loaded (or confirmed missing), we use in-memory state.
- Snapshot is written as a side effect of refresh, only if work was done. No configuration, no "every N events" policy.
- The view does NOT own a reference to EventLog. It borrows it during `refresh`/`rebuild`. This keeps ownership simple.
- `ReduceFn<S>` is a plain function pointer, not a closure or trait object. This keeps things simple and `Copy`-able.
- The reducer receives owned state and returns owned state: `fn(S, &Event) -> S`. This allows mutation with `mut` parameter binding in the function body.
- Unknown event types should be handled by the catch-all `_ => state` arm in the reducer. The view doesn't enforce this — it's a convention.

## Files

| File | Action |
|------|--------|
| `src/view.rs` | Create |
| `src/lib.rs` | Update — re-export `View`, `ReduceFn` |
| `tests/common/mod.rs` | Add `counter_reducer`, `todo_reducer`, `TodoState`, `TodoItem` |
| `tests/view_tests.rs` | Create |

## Test Helpers Addition (`tests/common/mod.rs`)

```rust
use eventfold::{Event, View};
use serde::{Serialize, Deserialize};

pub fn counter_reducer(state: u64, _event: &Event) -> u64 {
    state + 1
}

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub next_id: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

pub fn todo_reducer(mut state: TodoState, event: &Event) -> TodoState {
    match event.event_type.as_str() {
        "todo_added" => {
            state.items.push(TodoItem {
                id: state.next_id,
                text: event.data["text"].as_str().unwrap_or("").to_string(),
                done: false,
            });
            state.next_id += 1;
        }
        "todo_completed" => {
            let id = event.data["id"].as_u64().unwrap_or(0);
            if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                item.done = true;
            }
        }
        "todo_deleted" => {
            let id = event.data["id"].as_u64().unwrap_or(0);
            state.items.retain(|i| i.id != id);
        }
        _ => {}
    }
    state
}
```

## Acceptance Criteria

1. **Fresh view, empty log:** Refresh returns `S::default()`
2. **Fresh view, populated log:** Append 5 events, create new view, refresh → state reflects all 5 events
3. **Incremental refresh:** Append 3, refresh, append 2 more, refresh → state reflects all 5
4. **No-op refresh:** Refresh with no new events returns same state, does not write snapshot to disk
5. **Snapshot persistence:** Refresh, drop view, create new view with same name → loads snapshot, refresh only processes new events
6. **state() no I/O:** Calling `state()` without `refresh()` returns default. Calling `state()` after `refresh()` returns current state. Neither performs I/O.
7. **Rebuild:** Append events, refresh, then `rebuild()` → produces same state as fresh view
8. **Rebuild deletes snapshot:** After rebuild, snapshot file reflects full replay state
9. **Idempotent refresh:** Refresh twice with no new events — same state, no extra disk writes
10. **Counter reducer:** View with `counter_reducer` over N events → state is N
11. **Todo reducer:** View with `todo_reducer` → correct state after add/complete/delete events
12. **Two views, different reducers:** Same log, counter view and todo view → both produce correct independent states
13. **Independent snapshots:** Two views have separate snapshot files, refreshing one doesn't affect the other
14. **Late registration:** Append events, then create a new view → first refresh replays full history
15. **Cargo builds and all tests pass**

## Test Plan (`tests/view_tests.rs`)

- `test_fresh_view_empty_log` — default state
- `test_fresh_view_populated_log` — append then refresh
- `test_incremental_refresh` — append, refresh, append, refresh
- `test_no_op_refresh` — refresh twice, check snapshot mtime unchanged
- `test_snapshot_persistence` — refresh, drop, recreate, refresh with new events
- `test_state_no_io` — verify state() returns without reading files
- `test_rebuild` — append, refresh, rebuild, compare states
- `test_rebuild_deletes_snapshot` — verify snapshot is rewritten after rebuild
- `test_idempotent_refresh` — refresh twice, same result
- `test_counter_reducer` — N events → state is N
- `test_todo_add` — add events, verify items
- `test_todo_complete` — add + complete, verify done flag
- `test_todo_delete` — add + delete, verify removal
- `test_two_views_different_reducers` — counter + todo on same log
- `test_independent_snapshots` — verify separate files
- `test_late_view_creation` — append first, create view after
