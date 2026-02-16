use crate::state::{StatsState, TodoState};
use leptos::prelude::*;

/// Wrapper to mark `EventLog` as `Send`.
///
/// All concrete fields of `EventLog` are `Send`. The only non-Send field is
/// `Box<dyn ViewOps>`, but every concrete `ViewOps` implementation (`View<S>`
/// where `S: Send`) is `Send`. The missing `Send` bound on the `ViewOps` trait
/// object is the sole obstacle. Access is always synchronized via `Mutex`.
#[cfg(feature = "ssr")]
pub struct SendEventLog(pub eventfold::EventLog);

#[cfg(feature = "ssr")]
// SAFETY: See doc comment on SendEventLog above.
unsafe impl Send for SendEventLog {}

#[cfg(feature = "ssr")]
pub type AppLog = std::sync::Arc<std::sync::Mutex<SendEventLog>>;

#[cfg(feature = "ssr")]
fn use_eventfold() -> Result<AppLog, ServerFnError> {
    use_context::<AppLog>().ok_or_else(|| ServerFnError::new("EventLog not found in context"))
}

#[server]
pub async fn get_todos() -> Result<TodoState, ServerFnError> {
    let log = use_eventfold()?;
    let mut log = log.lock().expect("EventLog lock poisoned");
    log.0.refresh_all()?;
    let state: &TodoState = log.0.view("todos")?;
    Ok(state.clone())
}

#[server]
pub async fn get_stats() -> Result<StatsState, ServerFnError> {
    let log = use_eventfold()?;
    let mut log = log.lock().expect("EventLog lock poisoned");
    log.0.refresh_all()?;
    let state: &StatsState = log.0.view("stats")?;
    Ok(state.clone())
}

#[server]
pub async fn add_todo(text: String) -> Result<(), ServerFnError> {
    use eventfold::Event;
    use serde_json::json;

    let id = uuid::Uuid::new_v4().to_string();
    let created_at = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .expect("system clock before epoch")
        .as_secs();

    let log = use_eventfold()?;
    let mut log = log.lock().expect("EventLog lock poisoned");
    log.0.append(&Event::new(
        "todo_added",
        json!({ "id": id, "text": text, "created_at": created_at }),
    ))?;
    log.0.refresh_all()?;
    Ok(())
}

#[server]
pub async fn toggle_todo(id: String) -> Result<(), ServerFnError> {
    use eventfold::Event;
    use serde_json::json;

    let log = use_eventfold()?;
    let mut log = log.lock().expect("EventLog lock poisoned");
    log.0
        .append(&Event::new("todo_toggled", json!({ "id": id })))?;
    log.0.refresh_all()?;
    Ok(())
}

#[server]
pub async fn delete_todo(id: String) -> Result<(), ServerFnError> {
    use eventfold::Event;
    use serde_json::json;

    let log = use_eventfold()?;
    let mut log = log.lock().expect("EventLog lock poisoned");
    log.0
        .append(&Event::new("todo_deleted", json!({ "id": id })))?;
    log.0.refresh_all()?;
    Ok(())
}
