//! Changing a reducer and rebuilding a view.
//!
//! Shows how to evolve state shape: append events with a v1 reducer,
//! then rebuild with a v2 reducer that adds a priority field with a default.

use eventfold::{Event, EventLog, View};
use serde::{Deserialize, Serialize};
use serde_json::json;

// --- V1: just text ---

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct TodoV1 {
    items: Vec<ItemV1>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ItemV1 {
    text: String,
}

fn reducer_v1(mut state: TodoV1, event: &Event) -> TodoV1 {
    if event.event_type == "todo_added" {
        state.items.push(ItemV1 {
            text: event.data["text"].as_str().unwrap_or("").to_string(),
        });
    }
    state
}

// --- V2: text + priority ---

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
struct TodoV2 {
    items: Vec<ItemV2>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
struct ItemV2 {
    text: String,
    priority: String,
}

fn reducer_v2(mut state: TodoV2, event: &Event) -> TodoV2 {
    if event.event_type == "todo_added" {
        state.items.push(ItemV2 {
            text: event.data["text"].as_str().unwrap_or("").to_string(),
            priority: event.data["priority"]
                .as_str()
                .unwrap_or("normal")
                .to_string(),
        });
    }
    state
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;

    // Phase 1: append events and build v1 view
    {
        let mut log = EventLog::open(dir.path())?;
        log.append(&Event::new("todo_added", json!({"text": "buy milk"})))?;
        log.append(&Event::new("todo_added", json!({"text": "fix bug"})))?;

        let mut view = View::new("todos", reducer_v1, &dir.path().join("views"));
        view.refresh(&log.reader())?;
        println!("Before rebuild (v1): {:?}", view.state());
    }

    // Phase 2: rebuild with v2 reducer — old events get default priority
    {
        let log = EventLog::open(dir.path())?;
        let mut view = View::new("todos", reducer_v2, &dir.path().join("views"));
        view.rebuild(&log.reader())?;
        println!("After rebuild (v2):  {:?}", view.state());
    }

    Ok(())
}
