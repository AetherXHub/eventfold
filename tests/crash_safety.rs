mod common;

use common::{append_n, counter_reducer, dummy_event};
use eventfold::{EventLog, View};
use std::fs;
use std::io::Write;
use tempfile::tempdir;

/// Crash during append leaves a partial line at EOF.
/// Complete events before it must be intact, and the partial line is skipped.
#[test]
fn test_crash_during_append() {
    let dir = tempdir().unwrap();

    // Write 3 complete events
    {
        let mut log = EventLog::open(dir.path()).unwrap();
        append_n(&mut log, 3);
    }

    // Append a partial line (no trailing newline) — simulates crash mid-write
    {
        let mut file = fs::OpenOptions::new()
            .append(true)
            .open(dir.path().join("app.jsonl"))
            .unwrap();
        write!(file, r#"{{"event_type":"partial","data":{{}}"#).unwrap();
    }

    // Reopen — partial line at EOF should be skipped
    let mut log = EventLog::open(dir.path()).unwrap();

    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 3);
    assert_eq!(events[0].0.event_type, "event_0");
    assert_eq!(events[1].0.event_type, "event_1");
    assert_eq!(events[2].0.event_type, "event_2");

    // New appends succeed
    let event = dummy_event("new_event");
    log.append(&event).unwrap();
}

/// Crash during snapshot write leaves a .tmp file.
/// The .tmp is ignored; state is rebuilt from events.
#[test]
fn test_crash_during_snapshot_write() {
    let dir = tempdir().unwrap();

    // Set up a log with events
    {
        let mut log = EventLog::open(dir.path()).unwrap();
        append_n(&mut log, 5);
    }

    // Create a .tmp snapshot file (simulating crash during snapshot write)
    let tmp_path = dir.path().join("views/counter.snapshot.json.tmp");
    fs::write(
        &tmp_path,
        r#"{"state": 999, "offset": 999, "hash": "bad"}"#,
    )
    .unwrap();

    // No final snapshot file exists — .tmp should be ignored
    let log = EventLog::open(dir.path()).unwrap();
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log.reader()).unwrap();

    // State rebuilt correctly from events — bogus .tmp content (state=999) was not used
    assert_eq!(*view.state(), 5);
}

/// Crash after archive write but before active log truncation.
/// Events appear in both archive and active log (duplicated).
/// This is a known limitation — documented trade-off for simplicity.
#[test]
fn test_crash_after_archive_write_before_truncate() {
    let dir = tempdir().unwrap();

    // 1. Append events
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    // 2. Save active log content before rotation
    let log_content = fs::read(dir.path().join("app.jsonl")).unwrap();

    // 3. Rotate (archives + truncates + resets offsets)
    log.rotate().unwrap();
    drop(log);

    // 4. Restore active log content (simulating crash where truncation didn't happen)
    fs::write(dir.path().join("app.jsonl"), &log_content).unwrap();

    // 5. Verify: events are duplicated — archive has 5, active log has 5
    let log = EventLog::open(dir.path()).unwrap();
    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 10); // 5 archived + 5 in active = duplicated

    // A fresh view rebuild double-counts (known limitation)
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.rebuild(&log.reader()).unwrap();
    assert_eq!(*view.state(), 10); // double-counted
}

/// Crash after truncation but before snapshot offset reset.
/// Snapshot offset points into the now-empty active log.
/// Integrity check detects offset beyond EOF and triggers rebuild from archive.
#[test]
fn test_crash_after_truncate_before_offset_reset() {
    let dir = tempdir().unwrap();
    let snap_path = dir.path().join("views/counter.snapshot.json");

    // 1. Set up: append events, refresh view (creates snapshot with offset > 0)
    {
        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        append_n(&mut log, 5);
        log.refresh_all().unwrap();

        // Save the pre-rotation snapshot (has offset pointing into app.jsonl)
        let stale_snap = fs::read_to_string(&snap_path).unwrap();

        // Rotate normally (archives, truncates, resets offsets)
        log.rotate().unwrap();
        drop(log);

        // Restore stale snapshot (simulating crash after truncate but before offset reset)
        fs::write(&snap_path, &stale_snap).unwrap();
    }

    // 2. Reopen and verify recovery
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    log.refresh_all().unwrap();

    // Integrity check should detect offset beyond EOF, rebuild from archive
    assert_eq!(*log.view::<u64>("counter").unwrap(), 5);
}

/// Recovery from the same crash state is deterministic.
/// Re-corrupting the snapshot and reopening always produces the same result.
#[test]
fn test_crash_recovery_idempotent() {
    let dir = tempdir().unwrap();
    let snap_path = dir.path().join("views/counter.snapshot.json");

    // Set up crash state: archive has events, active log empty, stale snapshot
    let stale_snap: String;
    {
        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        append_n(&mut log, 5);
        log.refresh_all().unwrap();

        stale_snap = fs::read_to_string(&snap_path).unwrap();

        log.rotate().unwrap();
    }

    // Open and recover multiple times, re-corrupting each time
    let mut results = Vec::new();
    for _ in 0..3 {
        fs::write(&snap_path, &stale_snap).unwrap();

        let mut log = EventLog::builder(dir.path())
            .view::<u64>("counter", counter_reducer)
            .open()
            .unwrap();
        log.refresh_all().unwrap();
        results.push(*log.view::<u64>("counter").unwrap());
    }

    // All recoveries produce the same result
    assert!(results.iter().all(|&r| r == results[0]));
    assert_eq!(results[0], 5);
}

/// Partial lines at various positions in the file.
/// Complete lines before the partial are always read correctly.
#[test]
fn test_partial_line_various_positions() {
    // Case 1: Only a partial line (no complete lines before it)
    {
        let dir = tempdir().unwrap();
        let _ = EventLog::open(dir.path()).unwrap();

        // Overwrite with just a partial line
        fs::write(
            dir.path().join("app.jsonl"),
            r#"{"event_type":"x","data":{}"#,
        )
        .unwrap();

        let log = EventLog::open(dir.path()).unwrap();
        let events: Vec<_> = log
            .read_from(0)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(events.len(), 0);
    }

    // Case 2: One complete line followed by a partial
    {
        let dir = tempdir().unwrap();
        {
            let mut log = EventLog::open(dir.path()).unwrap();
            append_n(&mut log, 1);
        }
        {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(dir.path().join("app.jsonl"))
                .unwrap();
            write!(f, r#"{{"event_type":"partial"#).unwrap();
        }

        let log = EventLog::open(dir.path()).unwrap();
        let events: Vec<_> = log
            .read_from(0)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(events.len(), 1);
        assert_eq!(events[0].0.event_type, "event_0");
    }

    // Case 3: Multiple complete lines followed by a partial
    {
        let dir = tempdir().unwrap();
        {
            let mut log = EventLog::open(dir.path()).unwrap();
            append_n(&mut log, 5);
        }
        {
            let mut f = fs::OpenOptions::new()
                .append(true)
                .open(dir.path().join("app.jsonl"))
                .unwrap();
            write!(f, r#"{{"event_type":"partial","data":{{}}"#).unwrap();
        }

        let log = EventLog::open(dir.path()).unwrap();
        let events: Vec<_> = log
            .read_from(0)
            .unwrap()
            .collect::<Result<Vec<_>, _>>()
            .unwrap();
        assert_eq!(events.len(), 5);
        assert_eq!(events[0].0.event_type, "event_0");
        assert_eq!(events[4].0.event_type, "event_4");
    }
}
