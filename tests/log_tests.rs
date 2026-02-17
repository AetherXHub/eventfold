mod common;

use common::{append_n, dummy_event};
use eventfold::{line_hash, Event, EventLog};
use serde_json::json;
use std::io::Write;
use tempfile::tempdir;

#[test]
fn test_open_creates_directory() {
    let dir = tempdir().unwrap();
    let data_dir = dir.path().join("mydata");

    let _log = EventLog::open(&data_dir).unwrap();

    assert!(data_dir.exists(), "data directory should be created");
    assert!(data_dir.join("views").exists(), "views/ should be created");
    assert!(
        data_dir.join("app.jsonl").exists(),
        "app.jsonl should be created"
    );
}

#[test]
fn test_open_existing_directory() {
    let dir = tempdir().unwrap();
    let data_dir = dir.path().join("data");

    {
        let _log1 = EventLog::open(&data_dir).unwrap();
        // log1 dropped here, releasing the lock
    }
    let _log2 = EventLog::open(&data_dir).unwrap();
    // No error on second open after first is dropped
}

#[test]
fn test_append_single_event() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let event = dummy_event("test_event");
    log.append(&event).unwrap();

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(events[0].0.event_type, "test_event");
    assert_eq!(events[0].0.data, json!({"key": "value"}));
    assert_eq!(events[0].0.ts, 1000);
}

#[test]
fn test_append_multiple_events() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    append_n(&mut log, 10);

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 10);
    for (i, (event, _, _)) in events.iter().enumerate() {
        assert_eq!(event.event_type, format!("event_{i}"));
    }
}

#[test]
fn test_read_from_zero() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    append_n(&mut log, 5);

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 5);
    assert_eq!(events[0].0.event_type, "event_0");
    assert_eq!(events[4].0.event_type, "event_4");
}

#[test]
fn test_read_from_offset() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let mut offsets = Vec::new();
    for i in 0..5 {
        let event = dummy_event(&format!("event_{i}"));
        let result = log.append(&event).unwrap();
        offsets.push(result.start_offset);
    }

    // Read from the offset of the 3rd event (index 2)
    let events: Vec<_> = log
        .read_from(offsets[2])
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 3);
    assert_eq!(events[0].0.event_type, "event_2");
    assert_eq!(events[1].0.event_type, "event_3");
    assert_eq!(events[2].0.event_type, "event_4");
}

#[test]
fn test_byte_offset_correctness() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let mut offsets = Vec::new();
    for i in 0..5 {
        let event = dummy_event(&format!("event_{i}"));
        let result = log.append(&event).unwrap();
        offsets.push(result.start_offset);
    }

    // Each offset should seek to exactly that event
    for (i, &offset) in offsets.iter().enumerate() {
        let events: Vec<_> = log
            .read_from(offset)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();

        assert!(
            !events.is_empty(),
            "read_from offset {offset} should return events"
        );
        assert_eq!(
            events[0].0.event_type,
            format!("event_{i}"),
            "offset {offset} should point to event_{i}"
        );
    }
}

#[test]
fn test_offset_chaining() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    append_n(&mut log, 5);

    // Read first event, get its next_offset
    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    let (_, next_offset, _) = &events[0];

    // Use next_offset to skip past first event
    let remaining: Vec<_> = log
        .read_from(*next_offset)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(remaining.len(), 4);
    assert_eq!(remaining[0].0.event_type, "event_1");
}

#[test]
fn test_empty_log() {
    let dir = tempdir().unwrap();
    let log = EventLog::open(dir.path()).unwrap();

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(events.is_empty());
}

#[test]
fn test_hash_determinism() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    // Append two identical events (same content, same ts)
    let event = Event {
        event_type: "same".to_string(),
        data: json!({"x": 1}),
        ts: 5000,
        id: None,
        actor: None,
        meta: None,
    };
    log.append(&event).unwrap();
    log.append(&event).unwrap();

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_eq!(
        events[0].2, events[1].2,
        "identical event lines should produce identical hashes"
    );
}

#[test]
fn test_hash_differs_for_different_events() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let event_a = dummy_event("type_a");
    let event_b = dummy_event("type_b");
    log.append(&event_a).unwrap();
    log.append(&event_b).unwrap();

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 2);
    assert_ne!(
        events[0].2, events[1].2,
        "different event lines should produce different hashes"
    );
}

#[test]
fn test_line_hash_function() {
    let hash1 = line_hash(b"hello world");
    let hash2 = line_hash(b"hello world");
    let hash3 = line_hash(b"different");

    assert_eq!(hash1, hash2);
    assert_ne!(hash1, hash3);
    assert_eq!(hash1.len(), 16, "hex hash should be 16 characters");
}

#[test]
fn test_reopen_persistence() {
    let dir = tempdir().unwrap();

    // Open, append 3, close
    {
        let mut log = EventLog::open(dir.path()).unwrap();
        append_n(&mut log, 3);
    }

    // Reopen, verify 3 events exist, append 2 more
    {
        let mut log = EventLog::open(dir.path()).unwrap();

        let events: Vec<_> = log
            .read_from(0)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(events.len(), 3);

        let event3 = dummy_event("event_3");
        let event4 = dummy_event("event_4");
        log.append(&event3).unwrap();
        log.append(&event4).unwrap();

        let all_events: Vec<_> = log
            .read_from(0)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(all_events.len(), 5);
        assert_eq!(all_events[0].0.event_type, "event_0");
        assert_eq!(all_events[4].0.event_type, "event_4");
    }
}

#[test]
fn test_special_characters() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let event = Event {
        event_type: "special".to_string(),
        data: json!({
            "emoji": "Hello 🌍🦀",
            "newline": "line1\nline2",
            "quote": "He said \"hi\"",
            "unicode": "日本語",
            "backslash": "path\\to\\file"
        }),
        ts: 2000,
        id: None,
        actor: None,
        meta: None,
    };
    log.append(&event).unwrap();

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 1);
    let read_event = &events[0].0;
    assert_eq!(read_event.data["emoji"], "Hello 🌍🦀");
    assert_eq!(read_event.data["newline"], "line1\nline2");
    assert_eq!(read_event.data["quote"], "He said \"hi\"");
    assert_eq!(read_event.data["unicode"], "日本語");
    assert_eq!(read_event.data["backslash"], "path\\to\\file");
}

#[test]
fn test_partial_line_skipped() {
    let dir = tempdir().unwrap();

    // Append some valid events
    {
        let mut log = EventLog::open(dir.path()).unwrap();
        append_n(&mut log, 3);
    }

    // Manually write a partial line (no trailing newline)
    {
        let mut file = std::fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("app.jsonl"))
            .unwrap();
        write!(file, r#"{{"type":"partial","data":{{}},"ts":99"#).unwrap();
        // No trailing newline — simulates crash mid-write
    }

    // Reopen and read — partial line should be skipped
    let log = EventLog::open(dir.path()).unwrap();
    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(
        events.len(),
        3,
        "should read 3 complete events, skipping partial line"
    );
}

#[test]
fn test_read_from_end_of_file() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    append_n(&mut log, 5);

    // Get the offset past the last event
    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    let eof_offset = events.last().unwrap().1;

    // Read from EOF — should return nothing
    let events: Vec<_> = log
        .read_from(eof_offset)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert!(events.is_empty(), "reading from EOF should return nothing");
}

#[test]
fn test_append_returns_incrementing_offsets() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let r0 = log.append(&dummy_event("a")).unwrap();
    let r1 = log.append(&dummy_event("b")).unwrap();
    let r2 = log.append(&dummy_event("c")).unwrap();

    assert_eq!(r0.start_offset, 0, "first event should start at offset 0");
    assert!(
        r1.start_offset > r0.start_offset,
        "second offset should be greater than first"
    );
    assert!(
        r2.start_offset > r1.start_offset,
        "third offset should be greater than second"
    );
}

#[test]
fn test_append_result_start_offset() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let result = log.append(&dummy_event("first")).unwrap();
    assert_eq!(result.start_offset, 0, "first append to empty log has start_offset == 0");
}

#[test]
fn test_append_result_end_offset() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let event = dummy_event("test");
    let json = serde_json::to_string(&event).unwrap();
    let expected_end = json.len() as u64 + 1; // +1 for '\n'

    let result = log.append(&event).unwrap();
    assert_eq!(result.start_offset, 0);
    assert_eq!(result.end_offset, expected_end);
}

#[test]
fn test_append_result_consecutive() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let r1 = log.append(&dummy_event("a")).unwrap();
    let r2 = log.append(&dummy_event("b")).unwrap();

    assert_eq!(
        r2.start_offset, r1.end_offset,
        "second append's start_offset should equal first's end_offset"
    );
}

#[test]
fn test_append_result_hash_matches_read() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let result = log.append(&dummy_event("test")).unwrap();

    let events: Vec<_> = log
        .read_from(result.start_offset)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();

    assert_eq!(events.len(), 1);
    assert_eq!(
        events[0].2, result.line_hash,
        "hash from AppendResult should match hash from read_from"
    );
}

#[test]
fn test_append_result_hash_deterministic() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let event = dummy_event("same");
    let r1 = log.append(&event).unwrap();
    let r2 = log.append(&event).unwrap();

    assert_eq!(
        r1.line_hash, r2.line_hash,
        "same event appended twice should produce same hash"
    );
}

#[test]
fn test_paths_correct() {
    let dir = tempdir().unwrap();
    let log = EventLog::open(dir.path()).unwrap();

    assert_eq!(log.dir(), dir.path());
    assert_eq!(log.log_path(), dir.path().join("app.jsonl"));
    assert_eq!(log.archive_path(), dir.path().join("archive.jsonl.zst"));
    assert_eq!(log.views_dir(), dir.path().join("views"));
}
