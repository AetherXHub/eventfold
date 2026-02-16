mod common;

use common::{append_n, counter_reducer};
use eventfold::{Event, EventLog, Snapshot, View};
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_valid_snapshot_accepted() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();
    assert_eq!(*view.state(), 5);

    // Second refresh with no changes — snapshot should be accepted, not rebuilt
    append_n(&mut log, 3);
    view.refresh(&log).unwrap();
    assert_eq!(*view.state(), 8);
}

#[test]
fn test_offset_beyond_eof() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    // Create a view and refresh to generate snapshot
    {
        let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
        view.refresh(&log).unwrap();
        assert_eq!(*view.state(), 5);
    }

    // Truncate app.jsonl to 10 bytes (less than snapshot offset)
    let log_path = dir.path().join("app.jsonl");
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&log_path)
        .unwrap();
    file.set_len(10).unwrap();
    drop(file);

    // Reopen log and create fresh view — should detect offset beyond EOF and rebuild
    let log = EventLog::open(dir.path()).unwrap();
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    // The truncated file has partial data; state depends on what's parseable
    // The key assertion: it doesn't panic or return the old state of 5
    assert!(*state < 5);
}

#[test]
fn test_hash_mismatch() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    // Create view and refresh
    {
        let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
        view.refresh(&log).unwrap();
        assert_eq!(*view.state(), 5);
    }

    // Read the snapshot to get the offset
    let snapshot_path = log.views_dir().join("counter.snapshot.json");
    let snap_content = fs::read_to_string(&snapshot_path).unwrap();
    let snap: serde_json::Value = serde_json::from_str(&snap_content).unwrap();
    let _offset = snap["offset"].as_u64().unwrap();

    // Overwrite the last line before the offset in app.jsonl
    // Read the file, modify the last complete line, write back
    let log_path = dir.path().join("app.jsonl");
    let content = fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    // Replace the last line with different content
    let modified_event = Event::new("modified_event", json!({"tampered": true}));
    let modified_json = serde_json::to_string(&modified_event).unwrap();
    let mut new_content = String::new();
    for line in &lines[..lines.len() - 1] {
        new_content.push_str(line);
        new_content.push('\n');
    }
    new_content.push_str(&modified_json);
    new_content.push('\n');
    fs::write(&log_path, &new_content).unwrap();

    // Reopen log and create fresh view — should detect hash mismatch and rebuild
    let log = EventLog::open(dir.path()).unwrap();
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    // After rebuild from modified log, should have 5 events (the content is still 5 lines)
    assert_eq!(*state, 5);
}

#[test]
fn test_empty_log_nonzero_offset() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    // Create view and refresh
    {
        let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
        view.refresh(&log).unwrap();
        assert_eq!(*view.state(), 5);
    }

    // Truncate app.jsonl to empty
    let log_path = dir.path().join("app.jsonl");
    fs::write(&log_path, "").unwrap();

    // Reopen log and create fresh view — should detect and rebuild (empty = default)
    let log = EventLog::open(dir.path()).unwrap();
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 0); // empty log → default state
}

#[test]
fn test_offset_zero_always_valid() {
    let dir = tempdir().unwrap();
    let log = EventLog::open(dir.path()).unwrap();

    // Manually create a snapshot with offset 0
    let snapshot_path = log.views_dir().join("counter.snapshot.json");
    let snap = Snapshot {
        state: 42u64,
        offset: 0,
        hash: String::new(),
    };
    eventfold::snapshot::save(&snapshot_path, &snap).unwrap();

    // Create view and refresh — offset 0 should always be considered valid
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    // Snapshot with offset 0 is accepted (state = 42), no events to process
    assert_eq!(*state, 42);
}

#[test]
fn test_rebuild_correctness_after_integrity_failure() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 10);

    // Create view and refresh
    {
        let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
        view.refresh(&log).unwrap();
        assert_eq!(*view.state(), 10);
    }

    // Corrupt: write bogus snapshot with offset beyond EOF
    let snapshot_path = log.views_dir().join("counter.snapshot.json");
    let bogus_snap = Snapshot {
        state: 9999u64,
        offset: 999999,
        hash: "bogus".to_string(),
    };
    eventfold::snapshot::save(&snapshot_path, &bogus_snap).unwrap();

    // Create fresh view — should detect corruption and rebuild
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    // After rebuild, should correctly count all 10 events
    assert_eq!(*state, 10);
}

#[test]
fn test_manual_log_edit_detected() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 3);

    // Create view and refresh
    {
        let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
        view.refresh(&log).unwrap();
        assert_eq!(*view.state(), 3);
    }

    // Insert an extra line in the middle of app.jsonl
    let log_path = dir.path().join("app.jsonl");
    let content = fs::read_to_string(&log_path).unwrap();
    let lines: Vec<&str> = content.lines().collect();
    let mut new_content = String::new();
    for (i, line) in lines.iter().enumerate() {
        new_content.push_str(line);
        new_content.push('\n');
        if i == 1 {
            // Insert extra event after second line
            let extra = Event::new("inserted", json!({"extra": true}));
            let extra_json = serde_json::to_string(&extra).unwrap();
            new_content.push_str(&extra_json);
            new_content.push('\n');
        }
    }
    fs::write(&log_path, &new_content).unwrap();

    // Reopen log and create fresh view — should detect hash mismatch
    let log = EventLog::open(dir.path()).unwrap();
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    // After rebuild from modified log with 4 lines, should count 4
    assert_eq!(*state, 4);
}

#[test]
fn test_no_false_positives() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());

    // Many cycles of append + refresh — no corruption, no false rebuilds
    for batch in 1..=10 {
        append_n(&mut log, 5);
        let state = view.refresh(&log).unwrap();
        assert_eq!(*state, batch * 5);
    }

    // Drop and recreate the view — snapshot should load fine
    drop(view);
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 50);

    // One more batch after reload
    append_n(&mut log, 5);
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 55);
}
