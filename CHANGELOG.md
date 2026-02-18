# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [0.2.0] - 2026-02-18

### Added

- **Event metadata** — optional `id`, `actor`, and `meta` fields on `Event` for
  multi-user apps, audit trails, and event correlation (PRD-11)
- **Reader/Writer separation** — `EventReader` and `EventWriter` types for
  concurrent access patterns (PRD-12)
- **`AppendResult`** — append operations now return start/end byte offsets and a
  line hash for optimistic concurrency (PRD-13)
- **Conditional append** — `append_if` checks expected offset and hash before
  writing, returning `AppendConflict` on mismatch (PRD-14)
- **File locking** — `LockMode::Flock` (default) acquires an exclusive advisory
  lock on the active log; `LockMode::None` opts out (PRD-15)
- **Tail / new-event detection** — `EventReader::has_new_events` polls for
  changes without reading (PRD-16)
- **Blocking tail** — `EventReader::wait_for_events` uses filesystem notifications
  to block until new data arrives or a timeout elapses (PRD-17)
- `Debug` impls for `EventWriter`, `EventLog`, `EventLogBuilder`, `View<S>`
- `Eq` derives for `AppendConflict` and `AppendResult`
- Rustdoc examples on all public items
- `# Errors` and `# Panics` doc sections where applicable
- Dual MIT/Apache-2.0 license

### Changed

- `ViewOps` trait is now sealed — only `View<S>` may implement it
- Event builder methods (`with_id`, `with_actor`, `with_meta`) consume and
  return `self` for chaining
- Examples use `?` instead of `unwrap()`

## [0.1.0] - 2026-02-17

### Added

- **Event type and serialization** — `Event` struct with JSON Lines
  round-tripping via serde (PRD-01)
- **Event log core** — `EventLog::open`, `append`, `read_from` with byte-offset
  tracking (PRD-02)
- **Snapshot persistence** — atomic save/load/delete of derived state checkpoints
  with `Snapshot<S>` (PRD-03)
- **Views and reducers** — `View<S>` with `ReduceFn` for incremental state
  derivation from the event stream (PRD-04)
- **Integrity checking** — xxh64 hash verification on incremental reads to detect
  log tampering or corruption (PRD-05)
- **Log rotation and archival** — zstd-compressed archive files with transparent
  `read_full` replay across active + archived segments (PRD-06)
- **Builder API** — `EventLogBuilder` with view registry, max log size, and
  auto-rotation on open/append (PRD-07)
- **Crash safety** — partial-line recovery, atomic snapshot writes, and
  property-based tests for rotation/replay invariants (PRD-08)
- Examples: `counter`, `todos`, `time_travel` (PRD-09)
- Leptos web application example (`examples-leptos/todo-app`) (PRD-10)

[0.2.0]: https://github.com/AetherXHub/eventfold/compare/v0.1.0...v0.2.0
[0.1.0]: https://github.com/AetherXHub/eventfold/releases/tag/v0.1.0
