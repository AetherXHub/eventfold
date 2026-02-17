mod common;

use common::{
    append_n, counter_reducer, dummy_event, stats_reducer, todo_reducer, StatsState, TodoState,
};
use eventfold::{Event, EventLog};
use serde_json::json;
use tempfile::tempdir;

#[test]
fn test_builder_creates_directory() {
    let dir = tempdir().unwrap();
    let path = dir.path().join("subdir");
    let _log = EventLog::builder(&path).open().unwrap();

    assert!(path.exists());
    assert!(path.join("views").exists());
    assert!(path.join("app.jsonl").exists());
}

#[test]
fn test_builder_registers_views() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .unwrap();

    append_n(&mut log, 3);
    log.refresh_all().unwrap();

    let todos: &TodoState = log.view("todos").unwrap();
    let stats: &StatsState = log.view("stats").unwrap();
    assert!(todos.items.is_empty());
    assert_eq!(stats.event_count, 3);
}

#[test]
fn test_refresh_all() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .unwrap();

    let event = Event {
        event_type: "todo_added".to_string(),
        data: json!({"text": "buy milk"}),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    log.append(&event).unwrap();
    log.append(&dummy_event("something")).unwrap();

    log.refresh_all().unwrap();

    let todos: &TodoState = log.view("todos").unwrap();
    assert_eq!(todos.items.len(), 1);
    assert_eq!(todos.items[0].text, "buy milk");

    let stats: &StatsState = log.view("stats").unwrap();
    assert_eq!(stats.event_count, 2);
    assert_eq!(stats.last_event_type, "something");
}

#[test]
fn test_view_accessor_correct_type() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .unwrap();

    append_n(&mut log, 2);
    log.refresh_all().unwrap();

    let _todos: &TodoState = log.view("todos").unwrap();
    let _stats: &StatsState = log.view("stats").unwrap();
}

#[test]
fn test_view_accessor_nonexistent() {
    let dir = tempdir().unwrap();
    let log = EventLog::builder(dir.path()).open().unwrap();

    let result = log.view::<u64>("unknown");
    assert!(result.is_err());
}

#[test]
fn test_rotate_uses_registry() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    append_n(&mut log, 10);
    log.rotate().unwrap();

    assert_eq!(log.active_log_size().unwrap(), 0);
    assert!(log.archive_path().exists());
    assert_eq!(*log.view::<u64>("counter").unwrap(), 10);
}

#[test]
fn test_auto_rotation_on_append() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .max_log_size(500)
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    for i in 0..20 {
        let event = dummy_event(&format!("event_{i}"));
        log.append(&event).unwrap();
    }

    // Archive should exist (auto-rotation triggered)
    assert!(log.archive_path().exists());
    // Active log should be smaller than if all 20 events were there
    assert!(log.active_log_size().unwrap() < 500);
    // All events readable via read_full
    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 20);
}

#[test]
fn test_auto_rotation_on_open() {
    let dir = tempdir().unwrap();

    // First, create a log and add many events (no max_log_size)
    {
        let mut log = EventLog::open(dir.path()).unwrap();
        append_n(&mut log, 20);
    }

    // Reopen with max_log_size — should auto-rotate on open
    let log = EventLog::builder(dir.path())
        .max_log_size(500)
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    assert!(log.archive_path().exists());
    assert_eq!(log.active_log_size().unwrap(), 0);
    // Counter should reflect all events (rotation refreshes views first)
    assert_eq!(*log.view::<u64>("counter").unwrap(), 20);
}

#[test]
fn test_max_log_size_zero_disables() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .max_log_size(0)
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    append_n(&mut log, 50);

    assert!(!log.archive_path().exists());
    assert!(log.active_log_size().unwrap() > 0);
}

#[test]
fn test_full_lifecycle() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .max_log_size(10_000)
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .unwrap();

    // Append events
    let event1 = Event {
        event_type: "todo_added".to_string(),
        data: json!({"text": "buy milk"}),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    log.append(&event1).unwrap();

    let event2 = Event {
        event_type: "todo_added".to_string(),
        data: json!({"text": "walk dog"}),
        ts: 2000,
        id: None,
        actor: None,
        meta: None,
    };
    log.append(&event2).unwrap();

    // Refresh and check
    log.refresh_all().unwrap();
    let todos: &TodoState = log.view("todos").unwrap();
    assert_eq!(todos.items.len(), 2);
    let stats: &StatsState = log.view("stats").unwrap();
    assert_eq!(stats.event_count, 2);

    // Rotate
    log.rotate().unwrap();
    assert_eq!(log.active_log_size().unwrap(), 0);

    // Append more
    let event3 = Event {
        event_type: "todo_completed".to_string(),
        data: json!({"id": 0}),
        ts: 3000,
        id: None,
        actor: None,
        meta: None,
    };
    log.append(&event3).unwrap();

    // Refresh again
    log.refresh_all().unwrap();
    let todos: &TodoState = log.view("todos").unwrap();
    assert_eq!(todos.items.len(), 2);
    assert!(todos.items[0].done);
    let stats: &StatsState = log.view("stats").unwrap();
    assert_eq!(stats.event_count, 3);
}

#[test]
fn test_multiple_views() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .view::<u64>("counter", counter_reducer)
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .unwrap();

    append_n(&mut log, 5);
    log.refresh_all().unwrap();

    assert_eq!(*log.view::<u64>("counter").unwrap(), 5);
    assert!(log.view::<TodoState>("todos").unwrap().items.is_empty());
    assert_eq!(log.view::<StatsState>("stats").unwrap().event_count, 5);
}

#[test]
fn test_auto_rotation_multiple() {
    let dir = tempdir().unwrap();
    let mut log = EventLog::builder(dir.path())
        .max_log_size(300)
        .view::<u64>("counter", counter_reducer)
        .open()
        .unwrap();

    // Append many events — should trigger multiple auto-rotations
    for i in 0..50 {
        let event = dummy_event(&format!("event_{i}"));
        log.append(&event).unwrap();
    }

    // Refresh to get final state
    log.refresh_all().unwrap();
    assert_eq!(*log.view::<u64>("counter").unwrap(), 50);

    // All events readable via read_full
    let events: Vec<_> = log
        .read_full()
        .unwrap()
        .collect::<Result<Vec<_>, _>>()
        .unwrap();
    assert_eq!(events.len(), 50);
}

#[test]
fn test_builder_chaining() {
    let dir = tempdir().unwrap();
    // Verify the fluent API compiles and works
    let _log = EventLog::builder(dir.path())
        .max_log_size(1000)
        .view::<u64>("counter", counter_reducer)
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .unwrap();
}
