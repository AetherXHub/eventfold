#![allow(dead_code)]

use eventfold::{Event, EventLog};
use serde_json::json;

pub fn dummy_event(event_type: &str) -> Event {
    Event {
        event_type: event_type.to_string(),
        data: json!({"key": "value"}),
        ts: 1000,
    }
}

pub fn append_n(log: &mut EventLog, n: usize) {
    for i in 0..n {
        let event = dummy_event(&format!("event_{i}"));
        log.append(&event).unwrap();
    }
}
