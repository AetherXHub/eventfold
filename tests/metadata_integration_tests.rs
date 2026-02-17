mod common;

use eventfold::{Event, EventLog};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn test_append_and_read_with_metadata() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let event = Event::new("user_action", json!({"action": "click"}))
        .with_id("evt-001")
        .with_actor("user_42");
    log.append(&event).unwrap();

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 1);
    let read_event = &events[0].0;
    assert_eq!(read_event.event_type, "user_action");
    assert_eq!(read_event.id, Some("evt-001".to_string()));
    assert_eq!(read_event.actor, Some("user_42".to_string()));
    assert_eq!(read_event.data["action"], "click");
}

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
struct PerActorCount {
    user_a: u64,
    user_b: u64,
    unknown: u64,
}

fn per_actor_reducer(mut state: PerActorCount, event: &Event) -> PerActorCount {
    match event.actor.as_deref() {
        Some("user_a") => state.user_a += 1,
        Some("user_b") => state.user_b += 1,
        _ => state.unknown += 1,
    }
    state
}

#[test]
fn test_reducer_reads_actor() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<PerActorCount>("per_actor", per_actor_reducer)
        .open()
        .unwrap();

    log.append(&Event::new("click", json!({})).with_actor("user_a"))
        .unwrap();
    log.append(&Event::new("click", json!({})).with_actor("user_b"))
        .unwrap();
    log.append(&Event::new("click", json!({})).with_actor("user_a"))
        .unwrap();
    log.append(&Event::new("click", json!({})))
        .unwrap(); // no actor

    log.refresh_all().unwrap();
    let state: &PerActorCount = log.view("per_actor").unwrap();

    assert_eq!(state.user_a, 2);
    assert_eq!(state.user_b, 1);
    assert_eq!(state.unknown, 1);
}

#[test]
fn test_view_with_mixed_events() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<PerActorCount>("per_actor", per_actor_reducer)
        .open()
        .unwrap();

    // Old-style event (no metadata)
    log.append(&Event::new("click", json!({}))).unwrap();

    // New-style events (with metadata)
    log.append(&Event::new("click", json!({})).with_actor("user_a"))
        .unwrap();
    log.append(
        &Event::new("click", json!({}))
            .with_id("e3")
            .with_actor("user_b")
            .with_meta(json!({"session": "s1"})),
    )
    .unwrap();

    log.refresh_all().unwrap();
    let state: &PerActorCount = log.view("per_actor").unwrap();

    assert_eq!(state.user_a, 1);
    assert_eq!(state.user_b, 1);
    assert_eq!(state.unknown, 1);
}

#[test]
fn test_rotation_preserves_metadata() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<PerActorCount>("per_actor", per_actor_reducer)
        .open()
        .unwrap();

    // Append with metadata
    log.append(&Event::new("click", json!({})).with_id("e1").with_actor("user_a"))
        .unwrap();
    log.append(&Event::new("click", json!({})).with_id("e2").with_actor("user_b"))
        .unwrap();

    // Rotate — events move to archive
    log.rotate().unwrap();

    // Append more after rotation
    log.append(&Event::new("click", json!({})).with_id("e3").with_actor("user_a"))
        .unwrap();

    // Read full (archive + active) — metadata should survive
    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].0.id, Some("e1".to_string()));
    assert_eq!(events[0].0.actor, Some("user_a".to_string()));
    assert_eq!(events[1].0.id, Some("e2".to_string()));
    assert_eq!(events[1].0.actor, Some("user_b".to_string()));
    assert_eq!(events[2].0.id, Some("e3".to_string()));
    assert_eq!(events[2].0.actor, Some("user_a".to_string()));

    // Rebuild view from archive — should get correct state
    log.refresh_all().unwrap();
    let state: &PerActorCount = log.view("per_actor").unwrap();
    assert_eq!(state.user_a, 2);
    assert_eq!(state.user_b, 1);
    assert_eq!(state.unknown, 0);
}
