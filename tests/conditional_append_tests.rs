mod common;

use common::{counter_reducer, dummy_event};
use eventfold::{ConditionalAppendError, Event, EventLog, EventWriter};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn test_append_if_empty_log() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    let result = writer
        .append_if(&dummy_event("first"), 0, "")
        .unwrap();

    assert_eq!(result.start_offset, 0);
    assert!(result.end_offset > 0);

    let reader = writer.reader();
    let events: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0.event_type, "first");
}

#[test]
fn test_append_if_matches() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    let r1 = writer.append(&dummy_event("event_0")).unwrap();

    // Use r1's end_offset and line_hash as expected state
    let r2 = writer
        .append_if(&dummy_event("event_1"), r1.end_offset, &r1.line_hash)
        .unwrap();

    assert_eq!(r2.start_offset, r1.end_offset);

    let reader = writer.reader();
    let events: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0.event_type, "event_0");
    assert_eq!(events[1].0.event_type, "event_1");
}

#[test]
fn test_append_if_chain() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    // First append uses empty log convention
    let r1 = writer
        .append_if(&dummy_event("event_0"), 0, "")
        .unwrap();

    // Chain: each uses the previous result
    let r2 = writer
        .append_if(&dummy_event("event_1"), r1.end_offset, &r1.line_hash)
        .unwrap();

    let r3 = writer
        .append_if(&dummy_event("event_2"), r2.end_offset, &r2.line_hash)
        .unwrap();

    assert!(r3.end_offset > r2.end_offset);

    let reader = writer.reader();
    let events: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].0.event_type, "event_0");
    assert_eq!(events[1].0.event_type, "event_1");
    assert_eq!(events[2].0.event_type, "event_2");
}

#[test]
fn test_append_if_offset_mismatch() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    let _r1 = writer.append(&dummy_event("event_0")).unwrap();

    // Try to append_if with offset 0 — should fail since log is non-empty
    let err = writer
        .append_if(&dummy_event("event_1"), 0, "")
        .unwrap_err();

    match err {
        ConditionalAppendError::Conflict(conflict) => {
            assert_eq!(conflict.expected_offset, 0);
            assert!(
                conflict.actual_offset > 0,
                "actual offset should be non-zero after one append"
            );
            assert!(
                conflict.actual_hash.is_none(),
                "actual_hash should be None when offset check fails first"
            );
        }
        ConditionalAppendError::Io(e) => panic!("expected Conflict, got Io: {e}"),
    }
}

#[test]
fn test_append_if_hash_mismatch() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    let r1 = writer.append(&dummy_event("event_0")).unwrap();

    // Use correct offset but wrong hash
    let err = writer
        .append_if(&dummy_event("event_1"), r1.end_offset, "0000000000000000")
        .unwrap_err();

    match err {
        ConditionalAppendError::Conflict(conflict) => {
            assert_eq!(conflict.expected_offset, r1.end_offset);
            assert_eq!(conflict.actual_offset, r1.end_offset);
            assert_eq!(conflict.expected_hash, "0000000000000000");
            assert!(
                conflict.actual_hash.is_some(),
                "actual_hash should be populated when offset matches but hash differs"
            );
            assert_eq!(
                conflict.actual_hash.as_deref().unwrap(),
                r1.line_hash,
                "actual_hash should match the real last line hash"
            );
        }
        ConditionalAppendError::Io(e) => panic!("expected Conflict, got Io: {e}"),
    }
}

#[test]
fn test_append_if_no_write_on_conflict() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    writer.append(&dummy_event("event_0")).unwrap();

    let size_before = writer.active_log_size().unwrap();

    // This should fail — offset mismatch
    let _ = writer.append_if(&dummy_event("event_1"), 0, "");

    let size_after = writer.active_log_size().unwrap();
    assert_eq!(
        size_before, size_after,
        "file size should be unchanged after conflict"
    );

    let reader = writer.reader();
    let events: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(
        events.len(),
        1,
        "should still have only the original event"
    );
}

#[test]
fn test_append_if_result_matches_append() {
    // Compare AppendResult from append_if vs append for equivalent scenarios
    let dir_a = tempdir().unwrap();
    let dir_b = tempdir().unwrap();

    let mut writer_a = EventWriter::open(dir_a.path()).unwrap();
    let mut writer_b = EventWriter::open(dir_b.path()).unwrap();

    let event = dummy_event("test");

    let result_append = writer_a.append(&event).unwrap();
    let result_if = writer_b.append_if(&event, 0, "").unwrap();

    assert_eq!(
        result_append.start_offset, result_if.start_offset,
        "start_offset should match"
    );
    assert_eq!(
        result_append.end_offset, result_if.end_offset,
        "end_offset should match"
    );
    assert_eq!(
        result_append.line_hash, result_if.line_hash,
        "line_hash should match"
    );
}

#[test]
fn test_append_if_concurrent_simulation() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    // Initial event
    let r0 = writer.append(&dummy_event("initial")).unwrap();

    // Two "actors" both read the same state
    let actor_a_offset = r0.end_offset;
    let actor_a_hash = r0.line_hash.clone();
    let actor_b_offset = r0.end_offset;
    let actor_b_hash = r0.line_hash.clone();

    // Actor A writes first — succeeds
    let _ra = writer
        .append_if(&dummy_event("actor_a"), actor_a_offset, &actor_a_hash)
        .unwrap();

    // Actor B tries to write — should fail (offset has changed)
    let err = writer
        .append_if(&dummy_event("actor_b"), actor_b_offset, &actor_b_hash)
        .unwrap_err();

    match err {
        ConditionalAppendError::Conflict(conflict) => {
            assert_eq!(conflict.expected_offset, actor_b_offset);
            assert!(
                conflict.actual_offset > actor_b_offset,
                "actual offset should have advanced past actor B's expected offset"
            );
        }
        ConditionalAppendError::Io(e) => panic!("expected Conflict, got Io: {e}"),
    }

    // Only 2 events should exist: initial + actor_a
    let reader = writer.reader();
    let events: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0.event_type, "initial");
    assert_eq!(events[1].0.event_type, "actor_a");
}

#[test]
fn test_eventlog_append_if_delegates() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    // Append via EventLog::append_if on empty log
    let r1 = log.append_if(&dummy_event("event_0"), 0, "").unwrap();
    assert_eq!(r1.start_offset, 0);

    // Chain another
    let r2 = log
        .append_if(&dummy_event("event_1"), r1.end_offset, &r1.line_hash)
        .unwrap();
    assert_eq!(r2.start_offset, r1.end_offset);

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 2);
    assert_eq!(events[0].0.event_type, "event_0");
    assert_eq!(events[1].0.event_type, "event_1");
}

#[test]
fn test_eventlog_append_if_auto_rotation() {
    let dir = tempdir().unwrap();

    let mut log = EventLog::builder(dir.path())
        .max_log_size(1) // tiny threshold — triggers rotation after every append
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    // Conditional append on empty log
    let r1 = log
        .append_if(
            &Event::new("click", json!({"x": 1})),
            0,
            "",
        )
        .unwrap();

    // After auto-rotation, the active log should be empty (rotated away)
    assert_eq!(
        log.active_log_size().unwrap(),
        0,
        "active log should be empty after auto-rotation"
    );
    assert!(
        log.archive_path().exists(),
        "archive should exist after rotation"
    );

    // The result should still be valid
    assert_eq!(r1.start_offset, 0);
    assert!(r1.end_offset > 0);
}
