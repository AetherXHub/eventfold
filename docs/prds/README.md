# eventfold — Implementation PRDs

9 incremental PRDs that implement the full plan from `docs/plan.md`. Each PRD builds on the ones before it and produces a working, testable artifact.

## Dependency Graph

```
PRD 01: Event Type
  │
  ├──→ PRD 02: Event Log Core
  │      │
  │      ├──→ PRD 04: Views & Reducers ←── PRD 03: Snapshot Persistence
  │      │      │
  │      │      ├──→ PRD 05: Integrity Checking
  │      │      │
  │      │      └──→ PRD 06: Log Rotation & Archival
  │      │             │
  │      │             └──→ PRD 07: Builder API & View Registry
  │      │                    │
  │      │                    └──→ PRD 08: Crash Safety & Property Tests
  │      │                           │
  │      │                           └──→ PRD 09: Examples & Documentation
  │      │
  │      └── PRD 03: Snapshot Persistence
  │
  └── (foundation for all)
```

## Implementation Order

| # | PRD | What it produces | Key files |
|---|-----|------------------|-----------|
| 1 | [Event Type](01-event-type.md) | `Event` struct, serialization, test helpers | `src/event.rs`, `tests/event_tests.rs` |
| 2 | [Event Log Core](02-event-log.md) | `EventLog` with append/read on `app.jsonl` | `src/log.rs`, `tests/log_tests.rs` |
| 3 | [Snapshot Persistence](03-snapshot-persistence.md) | Atomic snapshot save/load/delete | `src/snapshot.rs`, `tests/snapshot_tests.rs` |
| 4 | [Views & Reducers](04-views-and-reducers.md) | `View<S>` with incremental refresh | `src/view.rs`, `tests/view_tests.rs` |
| 5 | [Integrity Checking](05-integrity-checking.md) | Hash verification, auto-rebuild on corruption | `tests/integrity_tests.rs` |
| 6 | [Log Rotation](06-log-rotation.md) | Zstd archive, `rotate()`, `read_full()` | `src/archive.rs`, `tests/rotation_tests.rs` |
| 7 | [Builder & Registry](07-builder-and-registry.md) | `EventLogBuilder`, auto-rotation, public API | `tests/builder_tests.rs` |
| 8 | [Crash & Property Tests](08-crash-safety-and-property-tests.md) | Crash simulation, proptest invariants | `tests/crash_safety.rs`, `tests/props.rs` |
| 9 | [Examples & Docs](09-examples-and-docs.md) | 6 examples, README, guide, rustdoc | `examples/`, `README.md`, `docs/guide.md` |

## Milestones

**After PRD 04:** The core system works end-to-end. You can append events, refresh views, and get derived state. No rotation or archival yet, but usable for simple cases.

**After PRD 07:** The full system is feature-complete. Builder API, view registry, auto-rotation, integrity checking — everything from the plan.

**After PRD 09:** The crate is ready for public release. Documentation, examples, and tests are comprehensive.

## Conventions

- All tests use `tempfile::tempdir()` for isolation
- Test helpers live in `tests/common/mod.rs`
- Each PRD lists its files, acceptance criteria, and test plan
- PRDs reference plan.md section names where applicable
