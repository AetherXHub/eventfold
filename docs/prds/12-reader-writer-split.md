# PRD 12: Reader/Writer Separation

## Summary

Split `EventLog` internals into `EventWriter` (exclusive append handle) and `EventReader` (cheap, cloneable read handle). `EventLog` becomes a convenience wrapper that owns both plus the view registry. This eliminates the `std::mem::take` workaround in `refresh_all()` and `rotate()`, and enables eventfold-es to hand out many readers while holding a single writer per stream.

## Prerequisites

- PRD 01–10 (all prior components)
- PRD 11 (event metadata) — can be implemented in parallel, no dependency

## Motivation

`EventLog` currently owns the append file handle, all read methods, and the view registry in a single struct. This creates two problems:

1. **Borrow checker workaround.** `View::refresh` needs `&EventLog` for reading, but `refresh_all()` and `rotate()` also need `&mut self` to manage views and truncate the file. The current solution is `std::mem::take(&mut self.views)` — temporarily moving views out to break the borrow. This is fragile and obscures intent.

2. **No independent readers.** eventfold-es needs to hand out many readers (to projections, subscriptions, query handlers) while holding a single writer. The current design doesn't support this — everything goes through `&EventLog`.

The read path already opens fresh file handles per call (`read_from` and `read_full` both call `File::open` internally, never using `self.file`). The split formalizes what the code already does.

## Scope

**In scope:**
- New `EventWriter` struct — owns the append `File` handle, `log_path`, `archive_path`, `max_log_size`
- New `EventReader` struct — owns `log_path`, `archive_path`; is `Clone + Send + Sync`
- Refactor `EventLog` to compose `EventWriter` + `EventReader` + `views`
- Move read methods (`read_from`, `read_full`, `read_line_hash_before`, `active_log_size`) to `EventReader`
- Move write methods (`append`, `rotate`) to `EventWriter`
- `EventWriter::reader()` — returns an `EventReader` pointing at the same paths
- `EventLog` delegates to inner writer/reader — **all existing public API preserved**
- Remove the `std::mem::take` workaround — views now take `&EventReader` instead of `&EventLog`
- Update `ViewOps` and `View<S>` to accept `&EventReader` (this is the "Decouple Views" change from the improvements doc — it falls out naturally here)

**Out of scope:**
- File locking (PRD 15)
- `AppendResult` return type change (PRD 13)
- Conditional append (PRD 14)
- Tail/poll primitives (PRD 16)

## Types

### EventWriter

```rust
/// Exclusive writer for a single event log file.
pub struct EventWriter {
    file: File,
    log_path: PathBuf,
    archive_path: PathBuf,
    views_dir: PathBuf,
    max_log_size: u64,
}

impl EventWriter {
    /// Open or create an event log directory for writing.
    ///
    /// Creates `dir/`, `dir/views/`, and `dir/app.jsonl` if they don't exist.
    /// Opens `app.jsonl` in append mode.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self>;

    /// Append an event to the log. Returns the byte offset where the event starts.
    /// May trigger auto-rotation if `max_log_size > 0`.
    pub fn append(&mut self, event: &Event) -> io::Result<u64>;

    /// Manually trigger log rotation.
    /// Callers must pass mutable views so rotation can refresh and reset them.
    pub fn rotate(
        &mut self,
        reader: &EventReader,
        views: &mut HashMap<String, Box<dyn ViewOps>>,
    ) -> io::Result<()>;

    /// Get a cloneable reader pointing at the same log paths.
    pub fn reader(&self) -> EventReader;

    /// Path to the data directory.
    pub fn dir(&self) -> &Path;

    /// Path to `app.jsonl`.
    pub fn log_path(&self) -> &Path;

    /// Path to `archive.jsonl.zst`.
    pub fn archive_path(&self) -> &Path;

    /// Path to the `views/` directory.
    pub fn views_dir(&self) -> &Path;

    /// Current size of `app.jsonl` in bytes.
    pub fn active_log_size(&self) -> io::Result<u64>;
}
```

Note: `rotate` on `EventWriter` takes `views` as a parameter because the writer alone doesn't own views. `EventLog::rotate()` passes its own views through.

### EventReader

```rust
/// Cheap, cloneable reader for an event log.
///
/// Opens fresh file handles per read call. Safe to use concurrently
/// with an `EventWriter` on the same log — completed lines are immutable,
/// and partial lines at EOF are detected and skipped.
#[derive(Debug, Clone)]
pub struct EventReader {
    log_path: PathBuf,
    archive_path: PathBuf,
}

impl EventReader {
    /// Create a reader pointing at the given log directory.
    pub fn new(dir: impl AsRef<Path>) -> Self;

    /// Read events from the active log starting at `offset`.
    /// Yields `(Event, next_byte_offset, line_hash)`.
    pub fn read_from(
        &self,
        offset: u64,
    ) -> io::Result<impl Iterator<Item = io::Result<(Event, u64, String)>>>;

    /// Read all events — archive first, then active log.
    /// Yields `(Event, line_hash)`.
    pub fn read_full(&self) -> io::Result<FullEventIter>;

    /// Hash of the event line just before `offset`. Returns `None` if
    /// `offset` is 0 or beyond the file.
    pub fn read_line_hash_before(&self, offset: u64) -> io::Result<Option<String>>;

    /// Current size of `app.jsonl` in bytes.
    pub fn active_log_size(&self) -> io::Result<u64>;

    /// Path to `app.jsonl`.
    pub fn log_path(&self) -> &Path;

    /// Path to `archive.jsonl.zst`.
    pub fn archive_path(&self) -> &Path;
}
```

### Updated EventLog

```rust
/// Convenience wrapper that owns a writer, reader, and view registry.
///
/// Preserves the full existing public API. For advanced use cases
/// (multiple readers, direct writer access), use `EventWriter` and
/// `EventReader` directly.
pub struct EventLog {
    writer: EventWriter,
    reader: EventReader,
    views: HashMap<String, Box<dyn ViewOps>>,
}
```

All existing `EventLog` methods delegate:

| Method | Delegates to |
|--------|-------------|
| `append(&mut self, event)` | `self.writer.append(event)` + auto-rotate check via `self.writer.rotate(&self.reader, &mut self.views)` |
| `read_from(&self, offset)` | `self.reader.read_from(offset)` |
| `read_full(&self)` | `self.reader.read_full()` |
| `read_line_hash_before(&self, offset)` | `self.reader.read_line_hash_before(offset)` |
| `active_log_size(&self)` | `self.reader.active_log_size()` |
| `refresh_all(&mut self)` | iterates `self.views`, calls `view.refresh_boxed(&self.reader)` — **no more `std::mem::take`** |
| `rotate(&mut self)` | `self.writer.rotate(&self.reader, &mut self.views)` |
| `view::<S>(&self, name)` | unchanged (reads from `self.views`) |
| `dir()`, `log_path()`, etc. | delegate to writer/reader |

New methods on `EventLog`:

```rust
/// Get a cloneable reader for this log.
pub fn reader(&self) -> EventReader;

/// Get a reference to the inner writer.
pub fn writer(&self) -> &EventWriter;

/// Get a mutable reference to the inner writer.
pub fn writer_mut(&mut self) -> &mut EventWriter;
```

### Updated ViewOps and View

```rust
pub trait ViewOps {
    fn refresh_boxed(&mut self, reader: &EventReader) -> io::Result<()>;
    fn reset_offset(&mut self) -> io::Result<()>;
    fn view_name(&self) -> &str;
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

impl<S> View<S> {
    pub fn refresh(&mut self, reader: &EventReader) -> io::Result<&S>;
    pub fn rebuild(&mut self, reader: &EventReader) -> io::Result<&S>;
}
```

### Updated EventLogBuilder

```rust
pub struct EventLogBuilder {
    dir: PathBuf,
    max_log_size: u64,
    view_factories: Vec<ViewFactory>,
}

impl EventLogBuilder {
    pub fn max_log_size(self, bytes: u64) -> Self;
    pub fn view<S>(self, name: &str, reducer: ReduceFn<S>) -> Self;
    pub fn open(self) -> io::Result<EventLog>;
}
```

`open()` now creates an `EventWriter`, derives an `EventReader` from it, creates views, and assembles the `EventLog`. Auto-rotation on open uses the reader+writer pattern.

## Implementation Details

### Eliminating `std::mem::take`

The core motivation. Currently:

```rust
// Before (current code)
pub fn refresh_all(&mut self) -> io::Result<()> {
    let mut views = std::mem::take(&mut self.views);
    let result = (|| {
        for view in views.values_mut() {
            view.refresh_boxed(self)?;  // self = &EventLog
        }
        Ok(())
    })();
    self.views = views;
    result
}
```

After the split, views take `&EventReader` which is a separate struct from the one being mutated:

```rust
// After
pub fn refresh_all(&mut self) -> io::Result<()> {
    for view in self.views.values_mut() {
        view.refresh_boxed(&self.reader)?;  // reader is not mutably borrowed
    }
    Ok(())
}
```

### Rotation

`EventWriter::rotate` needs to:
1. Refresh all views (using the reader)
2. Read and compress the active log
3. Truncate `self.file`
4. Reset all view offsets

Since the writer owns `self.file` and the reader provides read access, and views are passed in as `&mut HashMap`, there's no borrow conflict.

### Auto-rotation on append

`EventWriter::append` cannot call `rotate` on its own because it doesn't own views. Two options:

**Option A:** `EventWriter::append` returns a flag indicating rotation is needed, and `EventLog::append` handles it.
**Option B:** `EventWriter` has a standalone `rotate_log` that only does the file-level work (compress + truncate), and `EventLog::rotate` wraps it with view refresh/reset.

Option A is simpler — the writer's append checks the size and returns a needs-rotation indicator. `EventLog::append` then calls `self.rotate()` if needed.

```rust
// In EventWriter
pub(crate) fn append_raw(&mut self, event: &Event) -> io::Result<(u64, bool)> {
    let offset = /* seek, write, sync */;
    let needs_rotate = self.max_log_size > 0
        && self.active_log_size()? >= self.max_log_size;
    Ok((offset, needs_rotate))
}

// In EventLog
pub fn append(&mut self, event: &Event) -> io::Result<u64> {
    let (offset, needs_rotate) = self.writer.append_raw(event)?;
    if needs_rotate {
        self.rotate()?;
    }
    Ok(offset)
}
```

For direct `EventWriter` users (eventfold-es), `append` returns normally and rotation is the caller's responsibility.

### `EventReader` is `Send + Sync`

`EventReader` contains only `PathBuf` fields (which are `Send + Sync`), and opens fresh file handles per read. No mutable state, no `File` handle. Derive `Clone`, and `Send + Sync` come for free.

### Internal iterator types

`LogIterator` and `EventLineIter` stay as-is — they're internal and don't reference `EventLog`. They already work with `File`/`BufRead`, which is what `EventReader` methods will provide.

## Files

| File | Action |
|------|--------|
| `src/log.rs` | Major refactor — extract `EventWriter` and `EventReader`, refactor `EventLog` as wrapper |
| `src/view.rs` | Update — `ViewOps::refresh_boxed` and `View::refresh`/`rebuild` take `&EventReader` |
| `src/lib.rs` | Update — re-export `EventWriter`, `EventReader` |
| `tests/common/mod.rs` | Update if needed |
| `tests/log_tests.rs` | Update — adapt to new API, add `EventReader`/`EventWriter` tests |
| `tests/view_tests.rs` | Update — pass `&reader` instead of `&log` |
| `tests/rotation_tests.rs` | Update — adapt to new rotation API |
| `tests/builder_tests.rs` | Update — add tests for `reader()` method |
| `tests/integrity_tests.rs` | Update — pass `&reader` |
| `tests/crash_safety.rs` | Update — adapt to new internals |
| `tests/props.rs` | Update — adapt to new API |
| `examples/*.rs` | No change — they use `EventLog` which preserves its API |
| `examples-leptos/todo-app/` | No change — uses `EventLog` |

## Acceptance Criteria

1. **EventWriter opens and appends:** `EventWriter::open(dir)` creates directory structure, `append` writes events
2. **EventReader reads:** `reader.read_from(0)` and `reader.read_full()` return correct events
3. **EventReader is Clone + Send + Sync:** compiles with `fn assert_send<T: Send + Sync + Clone>() {}; assert_send::<EventReader>();`
4. **writer.reader() works:** returns a reader that can read events the writer has appended
5. **EventLog preserves API:** all existing `EventLog` method signatures unchanged
6. **EventLog delegates correctly:** append, read_from, read_full, refresh_all, rotate all produce identical results to pre-refactor behavior
7. **No std::mem::take:** `refresh_all` and `rotate` no longer use the workaround
8. **Auto-rotation still works:** `EventLog` with `max_log_size` auto-rotates on append
9. **Views accept &EventReader:** `View::refresh` and `ViewOps::refresh_boxed` take `&EventReader`
10. **View refresh produces same results:** identical state after refresh with reader vs old API
11. **Rotation with views:** `EventLog::rotate()` refreshes views, compresses, truncates, resets offsets
12. **Multiple readers:** two `EventReader` clones reading the same log concurrently produce identical results
13. **All existing tests pass** (after updating to new signatures)
14. **Cargo builds and all tests pass with `cargo clippy -- -D warnings`**

## Test Plan

### New Tests (`tests/reader_writer_tests.rs`)

- `test_writer_creates_directory` — `EventWriter::open` on fresh path creates dir structure
- `test_writer_append_and_reader_read` — write events with writer, read with reader from writer
- `test_reader_clone` — clone a reader, both clones read identical events
- `test_reader_send_sync` — compile-time check that `EventReader: Clone + Send + Sync`
- `test_reader_independent_of_writer` — construct `EventReader::new(dir)` without a writer, read existing log
- `test_writer_rotate_with_views` — writer rotates with views passed in, offsets reset, archive created
- `test_eventlog_delegates_append` — `EventLog::append` matches `EventWriter::append` behavior
- `test_eventlog_delegates_read` — `EventLog::read_from` matches `EventReader::read_from`
- `test_eventlog_refresh_no_mem_take` — `refresh_all` works without the old workaround (correctness check)
- `test_eventlog_reader_method` — `log.reader()` returns working reader

### Updated Existing Tests

All existing tests in `log_tests.rs`, `view_tests.rs`, `rotation_tests.rs`, `builder_tests.rs`, `integrity_tests.rs`, `crash_safety.rs`, and `props.rs` must continue to pass. Where tests reference internal details (like directly calling `view.refresh(&log)`), update to use `&reader`.
