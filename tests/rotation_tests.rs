mod common;

use common::{append_n, counter_reducer};
use eventfold::{EventLog, Snapshot, View};
use tempfile::tempdir;

#[test]
fn test_basic_rotation() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    append_n(&mut log, 10);

    log.rotate().unwrap();

    assert_eq!(log.active_log_size().unwrap(), 0);
    assert!(log.archive_path().exists());
}

#[test]
fn test_archive_contains_events() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    append_n(&mut log, 5);

    log.rotate().unwrap();

    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 5);
}

#[test]
fn test_view_offsets_reset_after_rotation() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    append_n(&mut log, 5);

    log.rotate().unwrap();

    let snap: Snapshot<u64> =
        eventfold::snapshot::load(&log.views_dir().join("counter.snapshot.json"))
            .unwrap()
            .unwrap();
    assert_eq!(snap.offset, 0);
}

#[test]
fn test_view_state_unchanged_after_rotation() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    append_n(&mut log, 5);

    log.refresh_all().unwrap();
    let state_before = *log.view::<u64>("counter").unwrap();

    log.rotate().unwrap();

    let state_after = *log.view::<u64>("counter").unwrap();
    assert_eq!(state_after, state_before);
    assert_eq!(state_after, 5);
}

#[test]
fn test_post_rotation_appends() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    append_n(&mut log, 5);

    log.rotate().unwrap();

    append_n(&mut log, 5);
    let events: Vec<_> = log
        .read_from(0)
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 5);
}

#[test]
fn test_post_rotation_refresh() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    append_n(&mut log, 5);

    log.rotate().unwrap();

    append_n(&mut log, 3);
    log.refresh_all().unwrap();
    assert_eq!(*log.view::<u64>("counter").unwrap(), 8);
}

#[test]
fn test_multiple_rotations() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    for _ in 0..3 {
        append_n(&mut log, 5);
        log.rotate().unwrap();
    }

    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 15);
}

#[test]
fn test_read_full_after_rotations() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    for _ in 0..3 {
        append_n(&mut log, 5);
        log.rotate().unwrap();
    }

    // Append more to active log (not rotated)
    append_n(&mut log, 3);

    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 18);
}

#[test]
fn test_new_view_after_rotation() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();
    append_n(&mut log, 5);
    log.rotate().unwrap();

    append_n(&mut log, 3);

    // Create a NEW view — no snapshot, should replay archive + active log
    let mut new_view: View<u64> = View::new("counter2", counter_reducer, log.views_dir());
    new_view.refresh(&log).unwrap();
    assert_eq!(*new_view.state(), 8);
}

#[test]
fn test_empty_log_rotation_noop() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    log.rotate().unwrap();

    assert!(!log.archive_path().exists());
    assert_eq!(log.active_log_size().unwrap(), 0);
}

#[test]
fn test_rotation_with_no_views() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    log.rotate().unwrap();

    assert!(log.archive_path().exists());
    assert_eq!(log.active_log_size().unwrap(), 0);
}

#[test]
fn test_full_replay_matches_incremental() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    append_n(&mut log, 5);
    log.rotate().unwrap();

    append_n(&mut log, 3);
    log.refresh_all().unwrap();
    let incremental_state = *log.view::<u64>("counter").unwrap();

    // Create new view (full replay via read_full)
    let mut full_view: View<u64> = View::new("full_counter", counter_reducer, log.views_dir());
    full_view.refresh(&log).unwrap();
    let full_state = *full_view.state();

    assert_eq!(incremental_state, full_state);
    assert_eq!(full_state, 8);
}

#[test]
fn test_read_full_no_archive() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 5);
}

#[test]
fn test_read_full_empty_everything() {
    let dir = tempdir().unwrap();
    let log = EventLog::open(dir.path()).unwrap();

    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 0);
}
