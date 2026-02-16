> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 08: Crash Safety & Property-Based Tests

## Summary

Add crash safety tests that simulate filesystem states resulting from crashes at various points in the lifecycle, and property-based tests that verify fundamental invariants hold for arbitrary event sequences.

## Prerequisites

- PRD 01–07 (full system implemented)

## Scope

**In scope:**
- Crash simulation tests (manipulate files to simulate crash aftermath, verify recovery)
- Property-based tests with `proptest` for reducer determinism, rotation invariance, snapshot equivalence, ordering, and multi-view consistency
- Test helpers for crash simulation

**Out of scope:**
- Actual process crash testing (fork + kill)
- Performance benchmarks
- Fuzzing

## Dependencies

Add to `Cargo.toml`:

```toml
[dev-dependencies]
tempfile = "3"
proptest = "1"
```

## Files

| File | Action |
|------|--------|
| `Cargo.toml` | Add proptest to dev-dependencies |
| `tests/crash_safety.rs` | Create |
| `tests/props.rs` | Create |

## Crash Safety Tests (`tests/crash_safety.rs`)

These tests don't crash the process. They set up the filesystem state that *would* result from a crash at each point, then verify the system recovers correctly.

### Test Cases

#### `test_crash_during_append`

**Setup:** Write a partial line to `app.jsonl` (valid JSON prefix but no trailing newline).

**Verify:**
- Reopen log, `read_from(0)` skips the partial line
- Previously complete events are intact
- New appends succeed and are readable

#### `test_crash_during_snapshot_write`

**Setup:** Write a `.tmp` snapshot file (simulating crash before rename completed). Leave no final snapshot file, or leave an older one.

**Verify:**
- Reopen, create view, refresh — old snapshot is used (`.tmp` is ignored)
- State is rebuilt correctly from old snapshot + new events
- `.tmp` file does not interfere

#### `test_crash_after_archive_write_before_truncate`

**Setup:**
1. Append events to `app.jsonl`
2. Manually compress `app.jsonl` and append to `archive.jsonl.zst`
3. Do NOT truncate `app.jsonl` (crash happened here)
4. Snapshots still have old offsets into `app.jsonl`

**Verify:**
- Events exist in both archive and active log (duplicated)
- On rebuild (read_full), events are duplicated — this is the expected crash aftermath
- A fresh view rebuild produces state with double-counted events (this is the known cost — document it)
- OR: implement dedup detection based on hash/timestamp (optional, document trade-off)

**Resolution approach:** Document that crash during rotation can cause duplicate events. The recommended recovery is: if you suspect a crash during rotation, delete all snapshots and manually check `read_full()` output. For the target use case (small apps, rare crashes), this is acceptable.

#### `test_crash_after_truncate_before_offset_reset`

**Setup:**
1. `app.jsonl` is empty (truncated)
2. `archive.jsonl.zst` has the events
3. Snapshot offsets still point to old positions in `app.jsonl`

**Verify:**
- Snapshot offset > 0 but file is empty → integrity check detects offset beyond EOF
- Triggers rebuild from `read_full()` (archive + empty log)
- Produces correct state

#### `test_crash_recovery_idempotent`

**Verify:** Opening the same crash-state directory multiple times and refreshing always produces the same result. Recovery is deterministic.

#### `test_partial_line_various_positions`

**Setup:** Partial lines at various positions — beginning of file, middle, end. Complete lines before the partial.

**Verify:** All complete lines are read correctly, partial line is skipped.

## Property-Based Tests (`tests/props.rs`)

### Strategies

```rust
use proptest::prelude::*;
use proptest::collection::vec;

fn arb_event_type() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("type_a".to_string()),
        Just("type_b".to_string()),
        Just("type_c".to_string()),
        Just("type_d".to_string()),
    ]
}

fn arb_event() -> impl Strategy<Value = Event> {
    (arb_event_type(), any::<u64>()).prop_map(|(t, ts)| {
        Event {
            event_type: t,
            data: json!({"value": ts % 100}),
            ts,
        }
    })
}

fn arb_event_sequence() -> impl Strategy<Value = Vec<Event>> {
    vec(arb_event(), 0..50)
}
```

### Property: Reducer Determinism

For any sequence of events, replaying the full log always produces the same state as incremental refreshes with arbitrary snapshot points.

```rust
proptest! {
    #[test]
    fn prop_reducer_determinism(events in arb_event_sequence()) {
        // 1. Full replay: fold all events through reducer from default state
        // 2. Incremental: append all, refresh at random points
        // 3. Assert: both produce identical final state
    }
}
```

### Property: Rotation Invariance

For any sequence of events with rotations at random points, the final state is identical to replaying all events without rotation.

```rust
proptest! {
    #[test]
    fn prop_rotation_invariance(
        events in arb_event_sequence(),
        rotation_points in vec(0..50usize, 0..5)
    ) {
        // 1. Append all events, no rotation → final state A
        // 2. Append events with rotations at specified points → final state B
        // 3. Assert: A == B
    }
}
```

### Property: Snapshot Equivalence

Deleting all snapshots and rebuilding every view produces identical state to incrementally maintained state.

```rust
proptest! {
    #[test]
    fn prop_snapshot_equivalence(events in arb_event_sequence()) {
        // 1. Append all events, refresh incrementally → state A
        // 2. Delete all snapshots, rebuild → state B
        // 3. Assert: A == B
    }
}
```

### Property: Event Ordering

Events read back from `read_full()` after arbitrary rotations are in the exact order they were appended.

```rust
proptest! {
    #[test]
    fn prop_event_ordering(
        events in vec(arb_event(), 1..30),
        rotation_points in vec(0..30usize, 0..3)
    ) {
        // 1. Append events with rotations at specified points
        // 2. read_full() → collect all events
        // 3. Assert: event types and timestamps match original sequence
    }
}
```

### Property: Multi-View Consistency

For any sequence of events, every view's state after `refresh_all()` matches a fresh replay with that view's reducer.

```rust
proptest! {
    #[test]
    fn prop_multi_view_consistency(events in arb_event_sequence()) {
        // 1. Open log with 2 views (counter + todo), append all events, refresh_all → states A1, A2
        // 2. Fold same events through each reducer independently → states B1, B2
        // 3. Assert: A1 == B1, A2 == B2
    }
}
```

## Acceptance Criteria

1. **All crash safety tests pass** — system recovers correctly from every simulated crash state
2. **All property tests pass** — fundamental invariants hold for arbitrary event sequences
3. **No false failures** — property tests are deterministic (seeded) and don't flake
4. **Crash tests document known limitations** — the rotation-crash duplicate event case is documented
5. **Cargo test runs all tests successfully**
