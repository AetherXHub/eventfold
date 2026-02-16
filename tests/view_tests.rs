mod common;

use common::{append_n, counter_reducer, todo_reducer, TodoState};
use eventfold::{Event, EventLog, View};
use serde_json::json;
use std::fs;
use tempfile::tempdir;

#[test]
fn test_fresh_view_empty_log() {
    let dir = tempdir().unwrap();
    let log = EventLog::open(dir.path()).unwrap();
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 0);
}

#[test]
fn test_fresh_view_populated_log() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 5);
}

#[test]
fn test_incremental_refresh() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 3);
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 3);

    append_n(&mut log, 2);
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 5);
}

#[test]
fn test_no_op_refresh() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 3);
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();

    let snapshot_path = log.views_dir().join("counter.snapshot.json");
    let mtime_before = fs::metadata(&snapshot_path).unwrap().modified().unwrap();

    // Small sleep to ensure mtime would differ if file were rewritten
    std::thread::sleep(std::time::Duration::from_millis(50));

    view.refresh(&log).unwrap();
    let mtime_after = fs::metadata(&snapshot_path).unwrap().modified().unwrap();
    assert_eq!(mtime_before, mtime_after);
}

#[test]
fn test_snapshot_persistence() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 3);

    {
        let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
        view.refresh(&log).unwrap();
        // view dropped here, snapshot persists on disk
    }

    // Append more events
    append_n(&mut log, 2);

    // Create a new view with the same name — should load snapshot
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    // Should reflect all 5 events (3 from snapshot + 2 new)
    assert_eq!(*state, 5);
}

#[test]
fn test_state_no_io() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());

    // state() before refresh returns default
    assert_eq!(*view.state(), 0);

    append_n(&mut log, 3);
    view.refresh(&log).unwrap();

    // state() after refresh returns current
    assert_eq!(*view.state(), 3);
}

#[test]
fn test_rebuild() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 5);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();
    assert_eq!(*view.state(), 5);

    let state = view.rebuild(&log).unwrap();
    assert_eq!(*state, 5);
}

#[test]
fn test_rebuild_deletes_snapshot() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 3);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    view.refresh(&log).unwrap();

    let snapshot_path = log.views_dir().join("counter.snapshot.json");
    let content_before = fs::read_to_string(&snapshot_path).unwrap();

    // Append more events, then rebuild
    append_n(&mut log, 2);
    view.rebuild(&log).unwrap();

    let content_after = fs::read_to_string(&snapshot_path).unwrap();
    // Snapshot should be rewritten with full replay (offset reflects all 5 events)
    assert_ne!(content_before, content_after);
    assert_eq!(*view.state(), 5);
}

#[test]
fn test_idempotent_refresh() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 4);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state1 = *view.refresh(&log).unwrap();
    let state2 = *view.refresh(&log).unwrap();
    assert_eq!(state1, state2);
    assert_eq!(state1, 4);
}

#[test]
fn test_counter_reducer() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    let n = 42;
    append_n(&mut log, n);

    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, n as u64);
}

#[test]
fn test_todo_add() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    for text in &["Buy milk", "Walk dog", "Read book"] {
        let event = Event::new("todo_added", json!({"text": text}));
        log.append(&event).unwrap();
    }

    let mut view: View<TodoState> = View::new("todos", todo_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();

    assert_eq!(state.items.len(), 3);
    assert_eq!(state.items[0].text, "Buy milk");
    assert_eq!(state.items[0].id, 0);
    assert_eq!(state.items[1].text, "Walk dog");
    assert_eq!(state.items[1].id, 1);
    assert_eq!(state.items[2].text, "Read book");
    assert_eq!(state.items[2].id, 2);
    assert_eq!(state.next_id, 3);
    assert!(state.items.iter().all(|i| !i.done));
}

#[test]
fn test_todo_complete() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let add = Event::new("todo_added", json!({"text": "Buy milk"}));
    log.append(&add).unwrap();

    let complete = Event::new("todo_completed", json!({"id": 0}));
    log.append(&complete).unwrap();

    let mut view: View<TodoState> = View::new("todos", todo_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();

    assert_eq!(state.items.len(), 1);
    assert!(state.items[0].done);
    assert_eq!(state.items[0].text, "Buy milk");
}

#[test]
fn test_todo_delete() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    let add1 = Event::new("todo_added", json!({"text": "Buy milk"}));
    log.append(&add1).unwrap();
    let add2 = Event::new("todo_added", json!({"text": "Walk dog"}));
    log.append(&add2).unwrap();

    let delete = Event::new("todo_deleted", json!({"id": 0}));
    log.append(&delete).unwrap();

    let mut view: View<TodoState> = View::new("todos", todo_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();

    assert_eq!(state.items.len(), 1);
    assert_eq!(state.items[0].text, "Walk dog");
    assert_eq!(state.items[0].id, 1);
}

#[test]
fn test_two_views_different_reducers() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();

    for text in &["Task A", "Task B"] {
        let event = Event::new("todo_added", json!({"text": text}));
        log.append(&event).unwrap();
    }

    let mut counter_view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let mut todo_view: View<TodoState> = View::new("todos", todo_reducer, log.views_dir());

    let count = counter_view.refresh(&log).unwrap();
    let todos = todo_view.refresh(&log).unwrap();

    assert_eq!(*count, 2);
    assert_eq!(todos.items.len(), 2);
    assert_eq!(todos.items[0].text, "Task A");
    assert_eq!(todos.items[1].text, "Task B");
}

#[test]
fn test_independent_snapshots() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 3);

    let mut view_a: View<u64> = View::new("view_a", counter_reducer, log.views_dir());
    let mut view_b: View<u64> = View::new("view_b", counter_reducer, log.views_dir());

    view_a.refresh(&log).unwrap();
    view_b.refresh(&log).unwrap();

    let snap_a = log.views_dir().join("view_a.snapshot.json");
    let snap_b = log.views_dir().join("view_b.snapshot.json");

    assert!(snap_a.exists());
    assert!(snap_b.exists());
    assert_ne!(snap_a, snap_b);

    // Append more, refresh only view_a
    append_n(&mut log, 2);
    view_a.refresh(&log).unwrap();

    assert_eq!(*view_a.state(), 5);
    assert_eq!(*view_b.state(), 3); // view_b not refreshed
}

#[test]
fn test_late_view_creation() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::open(dir.path()).unwrap();
    append_n(&mut log, 10);

    // Create view after events already exist
    let mut view: View<u64> = View::new("counter", counter_reducer, log.views_dir());
    let state = view.refresh(&log).unwrap();
    assert_eq!(*state, 10);
}
