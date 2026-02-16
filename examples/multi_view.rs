//! Same event log, two views: todo state and statistics.
//!
//! Demonstrates that different reducers over the same events produce
//! independent views with independent snapshots.

use eventfold::{Event, EventLog};
use serde::{Deserialize, Serialize};
use serde_json::json;

// --- Todo view ---

#[derive(Default, Clone, Serialize, Deserialize)]
struct TodoState {
    items: Vec<TodoItem>,
    next_id: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct TodoItem {
    id: u64,
    text: String,
    done: bool,
}

fn todo_reducer(mut state: TodoState, event: &Event) -> TodoState {
    match event.event_type.as_str() {
        "todo_added" => {
            state.items.push(TodoItem {
                id: state.next_id,
                text: event.data["text"].as_str().unwrap_or("").to_string(),
                done: false,
            });
            state.next_id += 1;
        }
        "todo_completed" => {
            let id = event.data["id"].as_u64().unwrap_or(0);
            if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                item.done = true;
            }
        }
        _ => {}
    }
    state
}

// --- Stats view ---

#[derive(Default, Clone, Serialize, Deserialize)]
struct StatsState {
    created: u64,
    completed: u64,
    deleted: u64,
}

fn stats_reducer(mut state: StatsState, event: &Event) -> StatsState {
    match event.event_type.as_str() {
        "todo_added" => state.created += 1,
        "todo_completed" => state.completed += 1,
        "todo_deleted" => state.deleted += 1,
        _ => {}
    }
    state
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut log = EventLog::builder(dir.path())
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()?;

    // Append events
    log.append(&Event::new("todo_added", json!({"text": "buy milk"})))?;
    log.append(&Event::new("todo_added", json!({"text": "write docs"})))?;
    log.append(&Event::new("todo_added", json!({"text": "fix bug"})))?;
    log.append(&Event::new("todo_completed", json!({"id": 0})))?;

    // Refresh both views from the same log
    log.refresh_all()?;

    let todos: &TodoState = log.view("todos")?;
    let completed = todos.items.iter().filter(|i| i.done).count();
    println!(
        "Todos: {} items ({} completed)",
        todos.items.len(),
        completed
    );

    let stats: &StatsState = log.view("stats")?;
    let rate = if stats.created > 0 {
        (stats.completed * 100) / stats.created
    } else {
        0
    };
    println!(
        "Stats: {} created, {} completed, {} deleted ({}% completion rate)",
        stats.created, stats.completed, stats.deleted, rate
    );

    Ok(())
}
