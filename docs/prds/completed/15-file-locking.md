> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 15: File Locking

## Summary

Add file-level locking to `EventWriter` so that two processes cannot concurrently open the same log directory for writing. Uses advisory `flock` locks via the `fs2` crate for cross-platform support. Readers do not acquire locks — append-only semantics make concurrent reads safe.

## Prerequisites

- PRD 12 (Reader/Writer Split) — `EventWriter` exists as a separate struct

## Motivation

eventfold currently has no file-level coordination. Two processes opening the same log directory will corrupt data — interleaved partial lines, concurrent rotations, etc. Even within eventfold-es (single process), locking is good hygiene so external tools (backup scripts, log inspectors) can't interfere with a running writer.

The locking strategy is simple: one exclusive lock on the writer, no locks on readers. This works because:
- Append-only files are safe to read concurrently — completed lines (with trailing newlines) are immutable once written.
- The existing partial-line detection in `LogIterator` handles reading during a concurrent write.
- Only the writer can modify the file, so only the writer needs to be exclusive.

## Scope

**In scope:**
- `LockMode` enum — `Flock` (default) or `None` (for tests / known single-access)
- `EventWriter::open` acquires exclusive `flock` by default
- `EventWriter::open_with_lock(dir, LockMode)` — explicit lock mode
- New dependency: `fs2` crate for cross-platform file locking
- `EventLogBuilder::lock_mode(LockMode)` — configure locking for the `EventLog` wrapper
- `EventReader` — no locks, no changes

**Out of scope:**
- Shared (read) locks on `EventReader` — not needed for correctness
- Lock files in a separate path — we lock the `app.jsonl` file directly
- Distributed locking / NFS advisory locks (flock is local-only, which matches eventfold's embedded design)

## Types

```rust
/// Controls file locking behavior for an `EventWriter`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LockMode {
    /// Acquire an exclusive advisory lock on `app.jsonl`.
    /// Prevents other processes from opening a writer on the same file.
    /// This is the default.
    #[default]
    Flock,

    /// No locking. Use when you know only one process accesses the log,
    /// or in test scenarios where multiple writers are intentionally used.
    None,
}
```

## Implementation Details

### Writer Lock Acquisition

```rust
use fs2::FileExt;

impl EventWriter {
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_lock(dir, LockMode::Flock)
    }

    pub fn open_with_lock(dir: impl AsRef<Path>, lock: LockMode) -> io::Result<Self> {
        // ... create directories, open file in append mode ...

        if lock == LockMode::Flock {
            file.try_lock_exclusive().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!("another writer holds the lock on {}: {e}", log_path.display()),
                )
            })?;
        }

        // ... continue with normal setup ...
    }
}
```

### Non-blocking by Default

`try_lock_exclusive()` is non-blocking — it returns an error immediately if the lock is held. This is preferable to blocking indefinitely, which could hang the process. Callers who want to wait can retry with backoff.

### Lock Lifetime

The `flock` is advisory and tied to the file descriptor. When `EventWriter` is dropped, the `File` is closed, and the lock is released automatically. No explicit unlock needed.

### Lock Survives Rotation

During rotation, `self.file.set_len(0)` truncates the file but does not close the file descriptor. The `flock` lock survives truncation — the lock is on the file descriptor, not the file contents.

### EventLogBuilder Integration

```rust
impl EventLogBuilder {
    pub fn lock_mode(mut self, mode: LockMode) -> Self {
        self.lock_mode = mode;
        self
    }
}
```

Default is `LockMode::Flock`. The builder passes this through to `EventWriter::open_with_lock`.

### Existing Tests

Most existing tests use `tempdir()` with unique directories, so lock contention is not an issue. Tests that deliberately need multiple writers on the same directory must use `LockMode::None`. A helper or builder method makes this easy.

### Platform Support

The `fs2` crate uses:
- Linux: `flock(fd, LOCK_EX | LOCK_NB)` — standard advisory locking
- macOS: same `flock` syscall
- Windows: `LockFileEx` with `LOCKFILE_EXCLUSIVE_LOCK | LOCKFILE_FAIL_IMMEDIATELY`

All platforms are well-tested in `fs2`.

## Dependencies

```toml
[dependencies]
fs2 = "0.4"
```

`fs2` is a minimal crate with no transitive dependencies beyond `libc` (Linux/macOS) and `windows-sys` (Windows). This is the same crate and version used by ferridyndb for its database file locking (`ferridyn-core/src/storage/lock.rs`), keeping our locking approach consistent across projects.

### Future: migrate to `std::fs::File::try_lock`

Rust is stabilizing native file locking on `std::fs::File` (tracking issue [#130994](https://github.com/rust-lang/rust/issues/130994)). Once `File::try_lock()` / `File::try_lock_shared()` land in stable, we should drop the `fs2` dependency and use std directly. The API is nearly identical — the migration will be mechanical.

## Files

| File | Action |
|------|--------|
| `Cargo.toml` | Update — add `fs2 = "0.4"` |
| `src/log.rs` | Update — add `LockMode`, update `EventWriter::open`, add `open_with_lock` |
| `src/lib.rs` | Update — re-export `LockMode` |
| `tests/locking_tests.rs` | Create |
| `tests/common/mod.rs` | Possibly update — ensure test helpers use `LockMode::None` if needed |

## Acceptance Criteria

1. **Default locks:** `EventWriter::open(dir)` acquires an exclusive lock
2. **Second writer fails:** opening a second `EventWriter` on the same directory returns an error (not a hang)
3. **Error is descriptive:** the error message mentions the lock and the file path
4. **LockMode::None skips locking:** `EventWriter::open_with_lock(dir, LockMode::None)` does not lock
5. **Lock released on drop:** after dropping the first writer, a second writer can open the same directory
6. **Lock survives rotation:** after `rotate()`, the lock is still held
7. **EventReader unaffected:** `EventReader` works with or without a writer lock held
8. **Builder integration:** `EventLogBuilder::lock_mode(LockMode::None)` produces an unlocked `EventLog`
9. **Existing tests pass** — no lock contention in isolated tempdirs
10. **Cargo builds and all tests pass with `cargo clippy -- -D warnings`**

## Test Plan (`tests/locking_tests.rs`)

- `test_writer_acquires_lock` — open writer, verify it holds the lock (try opening a second one, expect error)
- `test_second_writer_fails` — two `EventWriter::open` on the same dir, second fails with descriptive error
- `test_lock_released_on_drop` — open writer, drop it, open a new writer on the same dir, succeeds
- `test_lock_mode_none_allows_multiple` — two writers with `LockMode::None` on the same dir both succeed
- `test_lock_survives_rotation` — open writer, rotate, try opening second writer, still fails
- `test_reader_works_with_locked_writer` — writer holds lock, reader reads events successfully
- `test_reader_works_without_writer` — no writer exists, reader reads existing log
- `test_builder_lock_mode` — `EventLog::builder(dir).lock_mode(LockMode::None).open()` opens without locking
- `test_builder_default_locks` — `EventLog::builder(dir).open()` acquires lock (second open fails)
