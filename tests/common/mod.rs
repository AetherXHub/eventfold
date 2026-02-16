use eventfold::Event;
use serde_json::json;

pub fn dummy_event(event_type: &str) -> Event {
    Event {
        event_type: event_type.to_string(),
        data: json!({"key": "value"}),
        ts: 1000,
    }
}
