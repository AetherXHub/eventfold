> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 09: Examples & Documentation

## Summary

Create all example programs, README, rustdoc comments, and the concepts guide. This is the final PRD — it produces the public-facing surface of the crate.

## Prerequisites

- PRD 01–08 (full system implemented and tested)

## Scope

**In scope:**
- 6 example programs in `examples/`
- README.md
- API documentation (rustdoc on all public types/methods)
- Concepts guide (`docs/guide.md`)

**Out of scope:**
- Leptos web example (`examples-leptos/`) — separate project, separate PRD if desired
- Publishing to crates.io

## Examples

All examples use `fn main() -> Result<(), Box<dyn std::error::Error>>` and include comments explaining what's happening. Each should be runnable with `cargo run --example <name>`.

### `examples/todo_cli.rs` (~50 lines)

Minimal CLI todo app. The "hello world" of eventfold.

**Shows:** Define state, define reducer, open log with builder, append events, refresh, print state.

```
$ cargo run --example todo_cli
Added: buy milk
Added: write docs
Completed: buy milk
Todos:
  [x] buy milk
  [ ] write docs
```

### `examples/multi_view.rs`

Same event log, two views: todo state and statistics.

**Shows:** Same events, different reducers, independent snapshots.

```
$ cargo run --example multi_view
Todos: 3 items (1 completed)
Stats: 3 created, 1 completed, 0 deleted (33% completion rate)
```

### `examples/rebuild.rs`

Changing a reducer and rebuilding a view.

**Shows:** Append events with v1 reducer (just text), update to v2 reducer (text + priority with default), rebuild, verify new state shape.

```
$ cargo run --example rebuild
Before rebuild (v1): TodoV1 { items: [{text: "buy milk"}, {text: "fix bug"}] }
After rebuild (v2): TodoV2 { items: [{text: "buy milk", priority: "normal"}, {text: "fix bug", priority: "normal"}] }
```

### `examples/rotation.rs`

Manual and auto rotation.

**Shows:** Configure small max_log_size, append enough to trigger rotation, list directory to show archive appeared, verify continuity.

```
$ cargo run --example rotation
Appended 100 events...
Before rotation: app.jsonl = 5432 bytes, archive = none
After rotation: app.jsonl = 0 bytes, archive = 1234 bytes
Appended 10 more events...
State still correct: count = 110
```

### `examples/time_travel.rs`

Replaying to a specific point.

**Shows:** Append 20 events, read one by one reducing manually, stop at event 10, print state at that point.

```
$ cargo run --example time_travel
Full state (20 events): count = 20
State at event 10: count = 10
State at event 5: count = 5
```

### `examples/notes_cli.rs`

A richer CLI app: tagged notes with search.

**Shows:** Add notes with tags, list notes, filter by tag, tag statistics. Two views: notes_view and tags_view.

```
$ cargo run --example notes_cli
Added note: "Fix login bug" [bug, auth]
Added note: "Add dark mode" [feature, ui]
Added note: "Update deps" [maintenance]

All notes (3):
  1. Fix login bug [bug, auth]
  2. Add dark mode [feature, ui]
  3. Update deps [maintenance]

Notes tagged 'bug' (1):
  1. Fix login bug [bug, auth]

Tag stats:
  bug: 1
  auth: 1
  feature: 1
  ui: 1
  maintenance: 1
```

## README.md

Structure (follow plan exactly):

1. **One-liner:** "Your application state is a fold over an event log."
2. **What it is:** 3-4 sentences. Append-only, derived views, snapshots, single directory, no infrastructure.
3. **Quick example:** The todo app. ~30 lines, complete, runnable.
4. **Core concepts:** Events, reducers, views — 2-3 sentences each.
5. **Installation:** `cargo add eventfold`
6. **Features list:** Bullet points from the plan.
7. **When to use / when not to use.**
8. **Link to docs/guide.md**

## Rustdoc

Every public type, method, and function gets a doc comment. Follow Rust conventions:
- First line is a one-sentence summary
- Blank line, then details
- `# Examples` section with runnable doctests for all public methods

Cover all public items listed in the plan's Documentation section:
- `EventLog`, `EventLogBuilder`, `Event`, `View`, `Snapshot`, `ReduceFn`
- All public methods on each type

## Guide (`docs/guide.md`)

Follow the plan's outline:

1. How it works — lifecycle of an event, ASCII data flow diagrams
2. Writing reducers — best practices, patterns
3. Multiple views — same log, different lenses
4. Rotation and archival — step by step, configuration guidance
5. Schema evolution — new event types, changed state, changed semantics, deprecated events
6. Crash safety guarantees — what's guaranteed, what's not
7. Debugging — practical commands for inspecting log, snapshots, archive
8. Limitations — honest assessment

## Files

| File | Action |
|------|--------|
| `examples/todo_cli.rs` | Create |
| `examples/multi_view.rs` | Create |
| `examples/rebuild.rs` | Create |
| `examples/rotation.rs` | Create |
| `examples/time_travel.rs` | Create |
| `examples/notes_cli.rs` | Create |
| `README.md` | Create |
| `docs/guide.md` | Create |
| `src/lib.rs` | Add module-level rustdoc |
| `src/event.rs` | Add rustdoc to all public items |
| `src/log.rs` | Add rustdoc to all public items |
| `src/view.rs` | Add rustdoc to all public items |
| `src/snapshot.rs` | Add rustdoc to all public items |
| `src/archive.rs` | Add rustdoc to all public items |

## Acceptance Criteria

1. **All 6 examples compile:** `cargo build --examples` succeeds
2. **All examples run:** Each example runs and produces reasonable output
3. **README exists and is complete:** All sections from the plan are present
4. **Rustdoc builds clean:** `cargo doc --no-deps` produces no warnings
5. **Doctests pass:** `cargo test --doc` succeeds
6. **Guide covers all topics:** All 8 sections from the plan are present in guide.md
7. **Examples are self-contained:** Each example is a single file, understandable without reading source
8. **Cargo builds and all tests pass**
