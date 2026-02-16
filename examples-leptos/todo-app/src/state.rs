use serde::{Deserialize, Serialize};

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct TodoState {
    pub items: Vec<Todo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub created_at: u64,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct StatsState {
    pub total_created: u64,
    pub total_completed: u64,
    pub total_deleted: u64,
}

#[cfg(feature = "ssr")]
pub fn todo_reducer(mut state: TodoState, event: &eventfold::Event) -> TodoState {
    match event.event_type.as_str() {
        "todo_added" => {
            state.items.push(Todo {
                id: event.data["id"].as_str().unwrap_or("").to_string(),
                text: event.data["text"].as_str().unwrap_or("").to_string(),
                done: false,
                created_at: event.data["created_at"].as_u64().unwrap_or(0),
            });
        }
        "todo_toggled" => {
            let id = event.data["id"].as_str().unwrap_or("");
            if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                item.done = !item.done;
            }
        }
        "todo_deleted" => {
            let id = event.data["id"].as_str().unwrap_or("");
            state.items.retain(|i| i.id != id);
        }
        _ => {}
    }
    state
}

#[cfg(feature = "ssr")]
pub fn stats_reducer(mut state: StatsState, event: &eventfold::Event) -> StatsState {
    match event.event_type.as_str() {
        "todo_added" => state.total_created += 1,
        "todo_toggled" => state.total_completed += 1,
        "todo_deleted" => state.total_deleted += 1,
        _ => {}
    }
    state
}
