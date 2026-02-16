use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// An immutable event record stored in the log.
///
/// Events are serialized as single JSON lines in `app.jsonl`. The `data` field
/// is intentionally untyped ([`serde_json::Value`]) — the log has no opinion
/// about event shapes. Reducers give events meaning.
///
/// # Examples
///
/// ```
/// use eventfold::Event;
/// use serde_json::json;
///
/// let event = Event::new("user_clicked", json!({"button": "submit"}));
/// assert_eq!(event.event_type, "user_clicked");
/// assert!(event.ts > 0);
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    /// The event type identifier (e.g. `"todo_added"`, `"user_clicked"`).
    ///
    /// Serialized as `"type"` in JSON for brevity.
    #[serde(rename = "type")]
    pub event_type: String,

    /// Arbitrary JSON payload. The log does not validate this — reducers
    /// interpret it however they need.
    pub data: Value,

    /// Unix timestamp in seconds, auto-populated by [`Event::new`].
    pub ts: u64,
}

impl Event {
    /// Create a new event with the given type and data.
    ///
    /// The timestamp is set to the current time (seconds since Unix epoch).
    ///
    /// # Examples
    ///
    /// ```
    /// use eventfold::Event;
    /// use serde_json::json;
    ///
    /// let event = Event::new("page_view", json!({"url": "/home"}));
    /// assert_eq!(event.event_type, "page_view");
    /// assert_eq!(event.data["url"], "/home");
    /// ```
    pub fn new(event_type: &str, data: Value) -> Self {
        let ts = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs();
        Event {
            event_type: event_type.to_string(),
            data,
            ts,
        }
    }
}
