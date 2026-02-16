//! Minimal CLI todo app — the "hello world" of eventfold.

use eventfold::{Event, EventLog};
use serde::{Deserialize, Serialize};
use serde_json::json;

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
            let text = event.data["text"].as_str().unwrap_or("").to_string();
            state.items.push(TodoItem {
                id: state.next_id,
                text,
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

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut log = EventLog::builder(dir.path())
        .view::<TodoState>("todos", todo_reducer)
        .open()?;

    // Append some events
    log.append(&Event::new("todo_added", json!({"text": "buy milk"})))?;
    println!("Added: buy milk");

    log.append(&Event::new("todo_added", json!({"text": "write docs"})))?;
    println!("Added: write docs");

    log.append(&Event::new("todo_completed", json!({"id": 0})))?;
    println!("Completed: buy milk");

    // Refresh the view to fold all events into state
    log.refresh_all()?;

    // Read the derived state
    let todos: &TodoState = log.view("todos")?;
    println!("\nTodos:");
    for item in &todos.items {
        let check = if item.done { "x" } else { " " };
        println!("  [{}] {}", check, item.text);
    }

    Ok(())
}
