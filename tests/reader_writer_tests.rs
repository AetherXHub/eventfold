mod common;

use common::{append_n, counter_reducer, dummy_event};
use eventfold::{EventLog, EventReader, EventWriter, View};
use std::collections::HashMap;
use tempfile::tempdir;

#[test]
fn test_writer_creates_directory() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("subdir");
    let _writer = EventWriter::open(&path).unwrap();

    assert!(path.exists());
    assert!(path.join("views").exists());
    assert!(path.join("app.jsonl").exists());
}

#[test]
fn test_writer_append_and_reader_read() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    writer.append(&dummy_event("event_0")).unwrap();
    writer.append(&dummy_event("event_1")).unwrap();
    writer.append(&dummy_event("event_2")).unwrap();

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
fn test_reader_clone() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    writer.append(&dummy_event("event_0")).unwrap();
    writer.append(&dummy_event("event_1")).unwrap();

    let reader = writer.reader();
    let reader_clone = reader.clone();

    let events_a: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let events_b: Vec<_> = reader_clone
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events_a.len(), events_b.len());
    for (a, b) in events_a.iter().zip(events_b.iter()) {
        assert_eq!(a.0.event_type, b.0.event_type);
        assert_eq!(a.1, b.1);
        assert_eq!(a.2, b.2);
    }
}

#[test]
fn test_reader_send_sync() {
    fn assert_send_sync_clone<T: Send + Sync + Clone>() {}
    assert_send_sync_clone::<EventReader>();
}

#[test]
fn test_reader_independent_of_writer() {
    let dir = tempdir().unwrap();

    // Create log with writer, append events
    {
        let mut writer = EventWriter::open(dir.path()).unwrap();
        writer.append(&dummy_event("event_0")).unwrap();
        writer.append(&dummy_event("event_1")).unwrap();
    }

    // Construct reader independently (no writer)
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
fn test_writer_rotate_with_views() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();

    // Create views
    let mut views: HashMap<String, Box<dyn eventfold::ViewOps>> = HashMap::new();
    views.insert(
        "counter".to_string(),
        Box::new(View::new("counter", counter_reducer, writer.views_dir())),
    );

    // Append events
    for i in 0..5 {
        writer.append(&dummy_event(&format!("event_{i}"))).unwrap();
    }

    let reader = writer.reader();

    // Rotate
    writer.rotate(&reader, &mut views).unwrap();

    // Verify
    assert_eq!(writer.active_log_size().unwrap(), 0);
    assert!(writer.archive_path().exists());

    // View offset should be reset
    let snap: eventfold::Snapshot<u64> =
        eventfold::snapshot::load(&writer.views_dir().join("counter.snapshot.json"))
            .unwrap()
            .unwrap();
    assert_eq!(snap.offset, 0);
    assert_eq!(snap.state, 5);
}

#[test]
fn test_eventlog_delegates_append() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let offset = log.append(&dummy_event("test")).unwrap();
    assert_eq!(offset, 0);

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0.event_type, "test");
}

#[test]
fn test_eventlog_delegates_read() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    // Read via EventLog
    let events_log: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    // Read via EventReader
    let reader = log.reader();
    let events_reader: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events_log.len(), events_reader.len());
    for (a, b) in events_log.iter().zip(events_reader.iter()) {
        assert_eq!(a.0.event_type, b.0.event_type);
        assert_eq!(a.1, b.1);
        assert_eq!(a.2, b.2);
    }
}

#[test]
fn test_eventlog_refresh_no_mem_take() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    append_n(&mut log, 10);
    log.refresh_all().unwrap();

    assert_eq!(*log.view::<u64>("counter").unwrap(), 10);

    // Append more and refresh again — no mem::take needed
    append_n(&mut log, 5);
    log.refresh_all().unwrap();

    assert_eq!(*log.view::<u64>("counter").unwrap(), 15);
}

#[test]
fn test_eventlog_reader_method() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 3);

    let reader = log.reader();
    let events: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].0.event_type, "event_0");
    assert_eq!(events[2].0.event_type, "event_2");
}
