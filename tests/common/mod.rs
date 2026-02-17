#![allow(dead_code)]

use eventfold::Event;
use eventfold::EventLog;
use serde::{Deserialize, Serialize};
use serde_json::json;

pub fn dummy_event(event_type: &str) -> Event {
    Event {
        event_type: event_type.to_string(),
        data: json!({"key": "value"}),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    }
}

pub fn append_n(log: &mut EventLog, n: usize) {
    for i in 0..n {
        let event = dummy_event(&format!("event_{i}"));
        log.append(&event).unwrap();
    }
}

pub fn counter_reducer(state: u64, _event: &Event) -> u64 {
    state + 1
}

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TodoState {
    pub items: Vec<TodoItem>,
    pub next_id: u64,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct TodoItem {
    pub id: u64,
    pub text: String,
    pub done: bool,
}

pub fn todo_reducer(mut state: TodoState, event: &Event) -> TodoState {
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
        "todo_deleted" => {
            let id = event.data["id"].as_u64().unwrap_or(0);
            state.items.retain(|i| i.id != id);
        }
        _ => {}
    }
    state
}

#[derive(Default, Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct StatsState {
    pub event_count: u64,
    pub last_event_type: String,
}

pub fn stats_reducer(mut state: StatsState, event: &Event) -> StatsState {
    state.event_count += 1;
    state.last_event_type = event.event_type.clone();
    state
}
