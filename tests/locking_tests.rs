mod common;

use common::dummy_event;
use eventfold::{EventLog, EventReader, EventWriter, LockMode};
use tempfile::tempdir;

#[test]
fn test_writer_acquires_lock() {
    let dir = tempdir().unwrap();
    let _writer = EventWriter::open(dir.path()).unwrap();

    // A second writer on the same directory should fail
    let result = EventWriter::open(dir.path());
    assert!(result.is_err(), "second writer should fail to open");
    let err = result.err().unwrap();
    assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
}

#[test]
fn test_second_writer_fails() {
    let dir = tempdir().unwrap();
    let _writer1 = EventWriter::open(dir.path()).unwrap();

    let result = EventWriter::open(dir.path());
    assert!(result.is_err());
    let err = result.err().unwrap();
    let msg = err.to_string();
    assert!(
        msg.contains("another writer holds the lock"),
        "error should mention the lock: {msg}"
    );
    assert!(
        msg.contains("app.jsonl"),
        "error should mention the file path: {msg}"
    );
}

#[test]
fn test_lock_released_on_drop() {
    let dir = tempdir().unwrap();

    {
        let _writer = EventWriter::open(dir.path()).unwrap();
        // writer dropped here
    }

    // Should succeed now that the first writer is dropped
    let _writer2 = EventWriter::open(dir.path()).unwrap();
}

#[test]
fn test_lock_mode_none_allows_multiple() {
    let dir = tempdir().unwrap();
    let _writer1 = EventWriter::open_with_lock(dir.path(), LockMode::None).unwrap();
    let _writer2 = EventWriter::open_with_lock(dir.path(), LockMode::None).unwrap();
    // Both succeed — no locking
}

#[test]
fn test_lock_survives_rotation() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    // Append some events
    for i in 0..5 {
        writer
            .append(&dummy_event(&format!("event_{i}")))
            .unwrap();
    }

    // Rotate — truncates file but keeps file descriptor (and lock)
    let reader = writer.reader();
    let mut views = std::collections::HashMap::new();
    writer.rotate(&reader, &mut views).unwrap();

    // Second writer should still fail — lock survives rotation
    let result = EventWriter::open(dir.path());
    assert!(result.is_err(), "lock should survive rotation");
    assert_eq!(
        result.err().unwrap().kind(),
        std::io::ErrorKind::AlreadyExists
    );
}

#[test]
fn test_reader_works_with_locked_writer() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    writer.append(&dummy_event("event_0")).unwrap();
    writer.append(&dummy_event("event_1")).unwrap();

    // Reader can read while writer holds the lock
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
fn test_reader_works_without_writer() {
    let dir = tempdir().unwrap();

    // Create log with writer, append events, then drop writer
    {
        let mut writer = EventWriter::open(dir.path()).unwrap();
        writer.append(&dummy_event("event_0")).unwrap();
        writer.append(&dummy_event("event_1")).unwrap();
    }

    // No writer exists — reader should still work
    let reader = EventReader::new(dir.path());
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
fn test_builder_lock_mode() {
    let dir = tempdir().unwrap();

    let _log = EventLog::builder(dir.path())
        .lock_mode(LockMode::None)
        .open()
        .unwrap();

    // A second open with LockMode::None should also succeed
    let _log2 = EventLog::builder(dir.path())
        .lock_mode(LockMode::None)
        .open()
        .unwrap();
}

#[test]
fn test_builder_default_locks() {
    let dir = tempdir().unwrap();

    let _log = EventLog::builder(dir.path()).open().unwrap();

    // Default is Flock — second open should fail
    let result = EventLog::builder(dir.path()).open();
    assert!(result.is_err(), "default builder should acquire lock");
    assert_eq!(
        result.err().unwrap().kind(),
        std::io::ErrorKind::AlreadyExists
    );
}
