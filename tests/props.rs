mod common;

use common::{counter_reducer, stats_reducer, StatsState};
use eventfold::{Event, EventLog};
use proptest::prelude::*;
use serde_json::json;
use tempfile::tempdir;

fn arb_event_type() -> impl Strategy<Value = String> {
    prop_oneof![
        Just("type_a".to_string()),
        Just("type_b".to_string()),
        Just("type_c".to_string()),
        Just("type_d".to_string()),
    ]
}

fn arb_event() -> impl Strategy<Value = Event> {
    (arb_event_type(), any::<u64>()).prop_map(|(t, ts)| Event {
        event_type: t,
        data: json!({"value": ts % 100}),
        ts,
        id: None,
        actor: None,
        meta: None,
    })
}

fn arb_event_sequence() -> impl Strategy<Value = Vec<Event>> {
    proptest::collection::vec(arb_event(), 0..50)
}

// For any event sequence, folding manually produces the same state as
// appending to a log and refreshing a registered view.
proptest! {
    #[test]
    fn prop_reducer_determinism(events in arb_event_sequence()) {
        let dir = tempdir().unwrap();

        // Full replay: fold manually
        let mut manual_state = 0u64;
        for event in &events {
            manual_state = counter_reducer(manual_state, event);
        }

        // Incremental: append all, then refresh
        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        for event in &events {
            log.append(event).unwrap();
        }
        log.refresh_all().unwrap();
        let view_state = *log.view::<u64>("counter").unwrap();

        prop_assert_eq!(manual_state, view_state);
    }
}

// Rotating at arbitrary points does not change the final view state.
proptest! {
    #[test]
    fn prop_rotation_invariance(
        events in arb_event_sequence(),
        rotation_points in proptest::collection::vec(0..50usize, 0..5)
    ) {
        // Without rotation
        let dir_a = tempdir().unwrap();
        let mut log_a = EventLog::builder(dir_a.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        for event in &events {
            log_a.append(event).unwrap();
        }
        log_a.refresh_all().unwrap();
        let state_a = *log_a.view::<u64>("counter").unwrap();

        // With rotations at specified points
        let dir_b = tempdir().unwrap();
        let mut log_b = EventLog::builder(dir_b.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        let mut sorted_points: Vec<usize> = rotation_points
            .iter()
            .filter(|&&p| p < events.len())
            .copied()
            .collect();
        sorted_points.sort();
        sorted_points.dedup();

        for (i, event) in events.iter().enumerate() {
            log_b.append(event).unwrap();
            if sorted_points.contains(&i) {
                log_b.rotate().unwrap();
            }
        }
        log_b.refresh_all().unwrap();
        let state_b = *log_b.view::<u64>("counter").unwrap();

        prop_assert_eq!(state_a, state_b);
    }
}

// Deleting all snapshots and rebuilding produces identical state
// to incrementally maintained views.
proptest! {
    #[test]
    fn prop_snapshot_equivalence(events in arb_event_sequence()) {
        let dir = tempdir().unwrap();

        // Incremental
        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        for event in &events {
            log.append(event).unwrap();
        }
        log.refresh_all().unwrap();
        let state_a = *log.view::<u64>("counter").unwrap();
        drop(log);

        // Delete snapshot and rebuild
        let snap_path = dir.path().join("views/counter.snapshot.json");
        let _ = std::fs::remove_file(&snap_path);

        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        log.refresh_all().unwrap();
        let state_b = *log.view::<u64>("counter").unwrap();

        prop_assert_eq!(state_a, state_b);
    }
}

// Events read back via read_full() after arbitrary rotations
// preserve the exact order they were appended.
proptest! {
    #[test]
    fn prop_event_ordering(
        events in proptest::collection::vec(arb_event(), 1..30),
        rotation_points in proptest::collection::vec(0..30usize, 0..3)
    ) {
        let dir = tempdir().unwrap();
        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();

        let mut sorted_points: Vec<usize> = rotation_points
            .iter()
            .filter(|&&p| p < events.len())
            .copied()
            .collect();
        sorted_points.sort();
        sorted_points.dedup();

        for (i, event) in events.iter().enumerate() {
            log.append(event).unwrap();
            if sorted_points.contains(&i) {
                log.rotate().unwrap();
            }
        }

        let read_events: Vec<_> = log
            .read_full()
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        prop_assert_eq!(read_events.len(), events.len());
        for (original, (read, _hash)) in events.iter().zip(read_events.iter()) {
            prop_assert_eq!(&original.event_type, &read.event_type);
            prop_assert_eq!(original.ts, read.ts);
        }
    }
}

// Every registered view's state after refresh_all() matches
// a manual fold of the same events through that view's reducer.
proptest! {
    #[test]
    fn prop_multi_view_consistency(events in arb_event_sequence()) {
        let dir = tempdir().unwrap();

        // With registered views
        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .view::<StatsState>("stats", stats_reducer)
            .open()
            .unwrap();
        for event in &events {
            log.append(event).unwrap();
        }
        log.refresh_all().unwrap();
        let counter_state = *log.view::<u64>("counter").unwrap();
        let stats_state = log.view::<StatsState>("stats").unwrap().clone();

        // Manual fold
        let mut manual_counter = 0u64;
        let mut manual_stats = StatsState::default();
        for event in &events {
            manual_counter = counter_reducer(manual_counter, event);
            manual_stats = stats_reducer(manual_stats, event);
        }

        prop_assert_eq!(counter_state, manual_counter);
        prop_assert_eq!(stats_state, manual_stats);
    }
}
