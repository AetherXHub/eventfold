mod common;

use common::{append_n, counter_reducer};
use eventfold::{EventLog, Snapshot, View, ViewOps};
use tempfile::tempdir;

#[test]
fn test_basic_rotation() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 10);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();

    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

    assert_eq!(log.active_log_size().unwrap(), 0);
    assert!(log.archive_path().exists());
}

#[test]
fn test_archive_contains_events() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();
    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

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
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();
    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

    let snap: Snapshot<u64> =
        eventfold::snapshot::load(&log.views_dir().join("counter.snapshot.json"))
            .unwrap()
            .unwrap();
    assert_eq!(snap.offset, 0);
}

#[test]
fn test_view_state_unchanged_after_rotation() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();
    let state_before = *view.state();

    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

    assert_eq!(*view.state(), state_before);
    assert_eq!(*view.state(), 5);
}

#[test]
fn test_post_rotation_appends() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();
    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

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
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();
    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

    append_n(&mut log, 3);
    view.refresh(&log).unwrap();
    assert_eq!(*view.state(), 8);
}

#[test]
fn test_multiple_rotations() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());

    for _ in 0..3 {
        append_n(&mut log, 5);
        view.refresh(&log).unwrap();
        log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();
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
    let mut log = EventLog::open(dir.path()).unwrap();

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());

    for _ in 0..3 {
        append_n(&mut log, 5);
        view.refresh(&log).unwrap();
        log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();
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
    let mut log = EventLog::open(dir.path()).unwrap();

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    append_n(&mut log, 5);
    view.refresh(&log).unwrap();
    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

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

    log.rotate(&mut []).unwrap();

    assert!(!log.archive_path().exists());
    assert_eq!(log.active_log_size().unwrap(), 0);
}

#[test]
fn test_rotation_with_no_views() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    log.rotate(&mut []).unwrap();

    assert!(log.archive_path().exists());
    assert_eq!(log.active_log_size().unwrap(), 0);
}

#[test]
fn test_full_replay_matches_incremental() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());

    append_n(&mut log, 5);
    view.refresh(&log).unwrap();
    log.rotate(&mut [&mut view as &mut dyn ViewOps]).unwrap();

    append_n(&mut log, 3);
    view.refresh(&log).unwrap();
    let incremental_state = *view.state();

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
