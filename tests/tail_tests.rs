mod common;

use common::dummy_event;
use eventfold::{EventLog, EventWriter};
use tempfile::tempdir;

#[test]
fn test_has_new_events_empty_log() {
    let dir = tempdir().unwrap();
    let writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    assert!(
        !reader.has_new_events(0).unwrap(),
        "empty log should have no new events at offset 0"
    );
}

#[test]
fn test_has_new_events_after_append() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    writer.append(&dummy_event("event_0")).unwrap();

    assert!(
        reader.has_new_events(0).unwrap(),
        "should have new events after append"
    );
}

#[test]
fn test_has_new_events_at_current_offset() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    let result = writer.append(&dummy_event("event_0")).unwrap();

    assert!(
        !reader.has_new_events(result.end_offset).unwrap(),
        "should have no new events at end_offset"
    );
}

#[test]
fn test_has_new_events_after_multiple_appends() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    let r1 = writer.append(&dummy_event("event_0")).unwrap();
    let r2 = writer.append(&dummy_event("event_1")).unwrap();

    // At offset 0, there are new events
    assert!(reader.has_new_events(0).unwrap());

    // At first event's end, there are still new events (second event)
    assert!(reader.has_new_events(r1.end_offset).unwrap());

    // At second event's end, no new events
    assert!(!reader.has_new_events(r2.end_offset).unwrap());
}

#[test]
fn test_has_new_events_after_rotation() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    let r1 = writer.append(&dummy_event("event_0")).unwrap();

    // Before rotation, there are events
    assert!(reader.has_new_events(0).unwrap());

    // Rotate — truncates active log
    let mut views = std::collections::HashMap::new();
    writer.rotate(&reader, &mut views).unwrap();

    // After rotation, the pre-rotation offset is stale — no new events
    assert!(
        !reader.has_new_events(r1.end_offset).unwrap(),
        "should return false after rotation (log truncated)"
    );

    // At offset 0, also no new events (log is empty)
    assert!(
        !reader.has_new_events(0).unwrap(),
        "should return false for empty log after rotation"
    );
}

#[test]
fn test_active_log_size_empty() {
    let dir = tempdir().unwrap();
    let writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    assert_eq!(reader.active_log_size().unwrap(), 0);
}

#[test]
fn test_active_log_size_after_append() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    let r1 = writer.append(&dummy_event("event_0")).unwrap();
    assert_eq!(reader.active_log_size().unwrap(), r1.end_offset);

    let r2 = writer.append(&dummy_event("event_1")).unwrap();
    assert_eq!(reader.active_log_size().unwrap(), r2.end_offset);
}

#[test]
fn test_active_log_size_after_rotation() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    writer.append(&dummy_event("event_0")).unwrap();

    let mut views = std::collections::HashMap::new();
    writer.rotate(&reader, &mut views).unwrap();

    assert_eq!(
        reader.active_log_size().unwrap(),
        0,
        "active log should be 0 after rotation"
    );
}

#[test]
fn test_poll_loop_simulation() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    let mut offset = 0u64;
    let mut seen_events = Vec::new();

    // Append 3 events
    writer.append(&dummy_event("event_0")).unwrap();
    writer.append(&dummy_event("event_1")).unwrap();
    writer.append(&dummy_event("event_2")).unwrap();

    // Poll loop: catch up on all new events
    while reader.has_new_events(offset).unwrap() {
        for result in reader.read_from(offset).unwrap() {
            let (event, next_offset, _hash) = result.unwrap();
            seen_events.push(event.event_type.clone());
            offset = next_offset;
        }
    }

    assert_eq!(seen_events, vec!["event_0", "event_1", "event_2"]);

    // No more new events at current offset
    assert!(!reader.has_new_events(offset).unwrap());

    // Append 2 more
    writer.append(&dummy_event("event_3")).unwrap();
    writer.append(&dummy_event("event_4")).unwrap();

    // Poll loop again: catch up on new events
    while reader.has_new_events(offset).unwrap() {
        for result in reader.read_from(offset).unwrap() {
            let (event, next_offset, _hash) = result.unwrap();
            seen_events.push(event.event_type.clone());
            offset = next_offset;
        }
    }

    assert_eq!(
        seen_events,
        vec!["event_0", "event_1", "event_2", "event_3", "event_4"],
        "all events should be seen exactly once"
    );
}

#[test]
fn test_eventlog_has_new_events() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    assert!(!log.has_new_events(0).unwrap());

    let r1 = log.append(&dummy_event("event_0")).unwrap();
    assert!(log.has_new_events(0).unwrap());
    assert!(!log.has_new_events(r1.end_offset).unwrap());

    // Matches reader behavior
    let reader = log.reader();
    assert_eq!(
        log.has_new_events(0).unwrap(),
        reader.has_new_events(0).unwrap()
    );
    assert_eq!(
        log.has_new_events(r1.end_offset).unwrap(),
        reader.has_new_events(r1.end_offset).unwrap()
    );
}
