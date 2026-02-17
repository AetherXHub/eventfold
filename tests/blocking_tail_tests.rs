mod common;

use common::dummy_event;
use eventfold::{EventLog, EventWriter, WaitResult};
use std::time::{Duration, Instant};
use tempfile::tempdir;

#[test]
fn test_wait_returns_immediately_with_existing_data() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    writer.append(&dummy_event("event_0")).unwrap();

    let start = Instant::now();
    let result = reader
        .wait_for_events(0, Duration::from_secs(1))
        .unwrap();
    let elapsed = start.elapsed();

    assert!(
        matches!(result, WaitResult::NewData(_)),
        "should return NewData immediately"
    );
    assert!(
        elapsed < Duration::from_millis(100),
        "should return without delay, took {:?}",
        elapsed
    );
}

#[test]
fn test_wait_detects_new_append() {
    let dir = tempdir().unwrap();
    let writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    // Spawn a thread that appends after a short delay.
    let writer_dir = dir.path().to_path_buf();
    let handle = std::thread::spawn(move || {
        std::thread::sleep(Duration::from_millis(100));
        // We need a separate writer — drop the original first.
        // Instead, use the passed-in writer via a channel or just
        // write directly to the file.
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(writer_dir.join("app.jsonl"))
            .unwrap();
        let event = dummy_event("delayed_event");
        let json = serde_json::to_string(&event).unwrap();
        use std::io::Write;
        writeln!(file, "{json}").unwrap();
        file.sync_data().unwrap();
    });

    let start = Instant::now();
    let result = reader
        .wait_for_events(0, Duration::from_secs(5))
        .unwrap();
    let elapsed = start.elapsed();

    handle.join().unwrap();

    assert!(
        matches!(result, WaitResult::NewData(_)),
        "should detect the new append"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "should wake up well before the 5s timeout, took {:?}",
        elapsed
    );

    // Keep the writer alive so the lock is held until the end.
    drop(writer);
}

#[test]
fn test_wait_timeout() {
    let dir = tempdir().unwrap();
    let writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    let start = Instant::now();
    let result = reader
        .wait_for_events(0, Duration::from_millis(200))
        .unwrap();
    let elapsed = start.elapsed();

    assert_eq!(result, WaitResult::Timeout, "should timeout on quiet log");
    assert!(
        elapsed >= Duration::from_millis(180),
        "should wait approximately 200ms, took {:?}",
        elapsed
    );
    assert!(
        elapsed < Duration::from_millis(500),
        "should not overshoot timeout by much, took {:?}",
        elapsed
    );
}

#[test]
fn test_wait_new_data_size_correct() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    writer.append(&dummy_event("event_0")).unwrap();

    let result = reader
        .wait_for_events(0, Duration::from_secs(1))
        .unwrap();
    let actual_size = reader.active_log_size().unwrap();

    match result {
        WaitResult::NewData(size) => {
            assert_eq!(
                size, actual_size,
                "NewData size should match active_log_size()"
            );
        }
        WaitResult::Timeout => panic!("expected NewData, got Timeout"),
    }
}

#[test]
fn test_wait_read_after_detection() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    writer.append(&dummy_event("event_0")).unwrap();
    writer.append(&dummy_event("event_1")).unwrap();

    let result = reader
        .wait_for_events(0, Duration::from_secs(1))
        .unwrap();
    assert!(matches!(result, WaitResult::NewData(_)));

    // Read events after detection.
    let events: Vec<_> = reader
        .read_from(0)
        .unwrap()
        .map(|r| r.unwrap().0.event_type)
        .collect();

    assert_eq!(events, vec!["event_0", "event_1"]);
}

#[test]
fn test_wait_toctou_safety() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();

    // Append event, then immediately call wait_for_events with offset 0.
    // The event was appended before the call, so it should be detected
    // by the initial check (not missed by a TOCTOU race).
    writer.append(&dummy_event("event_0")).unwrap();

    let result = reader
        .wait_for_events(0, Duration::from_millis(200))
        .unwrap();

    assert!(
        matches!(result, WaitResult::NewData(_)),
        "should not miss data due to TOCTOU race"
    );
}

#[test]
fn test_wait_multiple_rounds() {
    let dir = tempdir().unwrap();
    let mut writer = EventWriter::open(dir.path()).unwrap();
    let reader = writer.reader();
    let mut offset = 0u64;
    let mut seen_events = Vec::new();

    // Round 1: append 2 events, wait and read.
    writer.append(&dummy_event("event_0")).unwrap();
    writer.append(&dummy_event("event_1")).unwrap();

    let result = reader
        .wait_for_events(offset, Duration::from_secs(1))
        .unwrap();
    assert!(matches!(result, WaitResult::NewData(_)));

    for result in reader.read_from(offset).unwrap() {
        let (event, next_offset, _hash) = result.unwrap();
        seen_events.push(event.event_type.clone());
        offset = next_offset;
    }

    assert_eq!(seen_events, vec!["event_0", "event_1"]);

    // No more data at current offset.
    let result = reader
        .wait_for_events(offset, Duration::from_millis(100))
        .unwrap();
    assert_eq!(result, WaitResult::Timeout);

    // Round 2: append 2 more, wait and read.
    writer.append(&dummy_event("event_2")).unwrap();
    writer.append(&dummy_event("event_3")).unwrap();

    let result = reader
        .wait_for_events(offset, Duration::from_secs(1))
        .unwrap();
    assert!(matches!(result, WaitResult::NewData(_)));

    for result in reader.read_from(offset).unwrap() {
        let (event, _next_offset, _hash) = result.unwrap();
        seen_events.push(event.event_type.clone());
    }

    assert_eq!(
        seen_events,
        vec!["event_0", "event_1", "event_2", "event_3"],
        "all events should be seen exactly once across rounds"
    );
}

#[test]
fn test_eventlog_wait_delegates() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    // Empty log — should timeout.
    let result = log
        .wait_for_events(0, Duration::from_millis(100))
        .unwrap();
    assert_eq!(result, WaitResult::Timeout);

    // Append, then wait — should return immediately.
    let r1 = log.append(&dummy_event("event_0")).unwrap();
    let result = log
        .wait_for_events(0, Duration::from_secs(1))
        .unwrap();
    assert!(matches!(result, WaitResult::NewData(_)));

    // At end offset — should timeout.
    let result = log
        .wait_for_events(r1.end_offset, Duration::from_millis(100))
        .unwrap();
    assert_eq!(result, WaitResult::Timeout);

    // Matches reader behavior.
    let reader = log.reader();
    let log_result = log
        .wait_for_events(0, Duration::from_millis(100))
        .unwrap();
    let reader_result = reader
        .wait_for_events(0, Duration::from_millis(100))
        .unwrap();
    assert_eq!(log_result, reader_result);
}
