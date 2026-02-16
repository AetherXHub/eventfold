> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 07: Builder API & View Registry

## Summary

Implement the `EventLogBuilder` and view registry pattern — the primary public API for eventfold. This ties everything together: views are registered before opening, the log knows about all its views (enabling rotation to refresh them), and auto-rotation triggers on append when the log exceeds a configured size.

## Prerequisites

- PRD 01–06 (all prior components)

## Scope

**In scope:**
- `EventLog::builder(dir)` — returns `EventLogBuilder`
- `EventLogBuilder::max_log_size(bytes)` — configure auto-rotation threshold
- `EventLogBuilder::view(name, reducer)` — register a view
- `EventLogBuilder::open()` — open log, create views, auto-rotate if needed
- `EventLog::append()` updated with auto-rotation check
- `EventLog::refresh_all()` — refresh every registered view
- `EventLog::view::<S>(name)` — access a registered view's current state
- `EventLog::rotate()` — now uses internal view registry (no external parameter)
- View storage using type-erased trait objects

**Out of scope:**
- Async API
- Multi-process coordination
- Event validation

## Design

### Type-Erased View Storage

The challenge: `View<S>` is generic over `S`, but `EventLog` needs to store views of different state types in a single collection. Solution: trait objects.

```rust
use std::any::Any;
use std::collections::HashMap;

pub struct EventLog {
    dir: PathBuf,
    log_path: PathBuf,
    archive_path: PathBuf,
    file: File,
    views_dir: PathBuf,
    views: HashMap<String, Box<dyn ViewOps>>,
    max_log_size: u64,  // 0 = disabled
}
```

The `ViewOps` trait (from PRD 06) is extended:

```rust
pub trait ViewOps {
    fn refresh_boxed(&mut self, log: &EventLog) -> io::Result<()>;
    fn reset_offset(&mut self);
    fn name(&self) -> &str;
    fn save_snapshot(&self) -> io::Result<()>;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}
```

The `as_any` methods allow downcasting back to `View<S>` when the user requests a specific view by type.

### Builder Pattern

```rust
pub struct EventLogBuilder {
    dir: PathBuf,
    max_log_size: u64,
    view_factories: Vec<Box<dyn FnOnce(&Path) -> Box<dyn ViewOps>>>,
}

impl EventLogBuilder {
    pub fn max_log_size(mut self, bytes: u64) -> Self {
        self.max_log_size = bytes;
        self
    }

    pub fn view<S>(mut self, name: &str, reducer: ReduceFn<S>) -> Self
    where
        S: Serialize + DeserializeOwned + Default + Clone + 'static,
    {
        let name = name.to_string();
        self.view_factories.push(Box::new(move |views_dir| {
            Box::new(View::new(&name, reducer, views_dir))
        }));
        self
    }

    pub fn open(self) -> io::Result<EventLog> {
        // 1. Create directory structure
        // 2. Open app.jsonl
        // 3. Create all views from factories
        // 4. If max_log_size > 0 and app.jsonl exceeds it, auto-rotate
        // 5. Return EventLog
    }
}
```

### Auto-Rotation on Append

```rust
impl EventLog {
    pub fn append(&mut self, event: &Event) -> io::Result<u64> {
        let offset = /* existing append logic */;

        if self.max_log_size > 0 {
            let size = self.active_log_size()?;
            if size >= self.max_log_size {
                self.rotate()?;
            }
        }

        Ok(offset)
    }
}
```

Auto-rotation happens **after** the append succeeds. The event is written, then rotation may trigger. This means the log can briefly exceed `max_log_size` by one event, which is fine.

### Public API

```rust
impl EventLog {
    /// Create a builder for configuring and opening an event log.
    pub fn builder(dir: impl AsRef<Path>) -> EventLogBuilder;

    /// Append an event to the log. May trigger auto-rotation.
    pub fn append(&mut self, event: &Event) -> io::Result<u64>;

    /// Refresh all registered views.
    pub fn refresh_all(&mut self) -> io::Result<()>;

    /// Get a reference to a registered view's current state.
    /// Panics if the view name doesn't exist or the type doesn't match.
    pub fn view<S: 'static>(&self, name: &str) -> io::Result<&S>;

    /// Manually trigger log rotation.
    pub fn rotate(&mut self) -> io::Result<()>;
}
```

### Usage Example

```rust
let mut app = EventLog::builder("./data")
    .max_log_size(10_000_000)
    .view::<TodoState>("todos", todo_reducer)
    .view::<StatsState>("stats", stats_reducer)
    .open()?;

app.append(&Event::new("todo_added", json!({"text": "buy milk"})))?;
app.refresh_all()?;

let todos: &TodoState = app.view("todos")?;
println!("{:?}", todos);
```

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Major update — add builder, view registry, auto-rotation, refresh_all, view accessor |
| `src/view.rs` | Extend `ViewOps` trait with `as_any` |
| `src/lib.rs` | Update — re-export builder, clean up public API |
| `tests/common/mod.rs` | Add `StatsState`, `stats_reducer` |
| `tests/builder_tests.rs` | Create |

## Implementation Details

### Borrow checker considerations

`rotate()` and `refresh_all()` need mutable access to views while also reading from the log. Since the views and log state live in the same struct, we need to be careful:

- `read_from` and `read_full` should open **separate file handles** (not use `self.file`), so they can be called while `self` is mutably borrowed for view updates.
- Alternatively, extract the log reading functionality into methods that only borrow the path/archive_path (not the full struct).

The recommended pattern: separate the "log reader" from the "log writer" internally.

### View accessor error handling

`view::<S>(name)` can fail in two ways:
1. View name not found → return `Err` with descriptive message
2. Type mismatch (wrong `S`) → this is a programming error. Use `panic!` with a clear message, or return `Err`. Prefer `Err` for robustness.

### Auto-rotation on open

When `open()` is called with `max_log_size > 0`, check if `app.jsonl` already exceeds the threshold. If so, rotate immediately. This handles the case where the previous process crashed before rotation could complete.

## Acceptance Criteria

1. **Builder creates directory:** `EventLog::builder("./new_dir").open()` creates the directory and files
2. **Builder registers views:** Views registered via builder are accessible via `view::<S>(name)`
3. **Builder max_log_size:** Setting max_log_size enables auto-rotation
4. **refresh_all:** Refreshes every registered view, all states are current
5. **view accessor:** Returns correct typed state for each registered view
6. **view accessor error:** Requesting nonexistent view name returns error
7. **rotate uses registry:** `rotate()` refreshes all registered views, resets all offsets
8. **Auto-rotation on append:** With small max_log_size, appending past threshold triggers rotation
9. **Auto-rotation on open:** Opening with existing oversized log triggers rotation
10. **max_log_size(0) disables:** No auto-rotation regardless of file size
11. **Full lifecycle:** builder → open → append → refresh_all → view → rotate → append → refresh_all → view — all correct
12. **Multiple views in builder:** Register 3+ views, all work independently
13. **Cargo builds and all tests pass**

## Test Helpers Addition

```rust
#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatsState {
    pub event_count: u64,
    pub last_event_type: String,
}

pub fn stats_reducer(mut state: StatsState, event: &Event) -> StatsState {
    state.event_count += 1;
    state.last_event_type = event.event_type.clone();
    state
}
```

## Test Plan (`tests/builder_tests.rs`)

- `test_builder_creates_directory` — open with new path, verify structure
- `test_builder_registers_views` — register todo + stats views, verify both accessible
- `test_refresh_all` — append events, refresh_all, verify both views updated
- `test_view_accessor_correct_type` — get TodoState from "todos", StatsState from "stats"
- `test_view_accessor_nonexistent` — request "unknown" → error
- `test_rotate_uses_registry` — rotate refreshes all views, resets offsets
- `test_auto_rotation_on_append` — max_log_size = 500, append until rotation triggers
- `test_auto_rotation_on_open` — create oversized log, reopen with max_log_size → rotates
- `test_max_log_size_zero_disables` — max_log_size(0), large log, no rotation
- `test_full_lifecycle` — complete usage scenario
- `test_multiple_views` — 3 views, all independent
- `test_auto_rotation_multiple` — small threshold, many events → multiple rotations, correct final state
- `test_builder_chaining` — all builder methods chain fluently
