use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

/// An immutable event record stored in the log.
///
/// Events are serialized as single JSON lines in `app.jsonl`. The `data` field
/// is intentionally untyped ([`serde_json::Value`]) — the log has no opinion
/// about event shapes. Reducers give events meaning.
///
/// Optional metadata fields (`id`, `actor`, `meta`) support multi-user
/// applications, audit trails, and event correlation. When `None`, these
/// fields are omitted from serialized output — existing logs without
/// metadata deserialize without error.
///
/// # Examples
///
/// ```
/// use eventfold::Event;
/// use serde_json::json;
///
/// // Simple event — no metadata
/// let event = Event::new("user_clicked", json!({"button": "submit"}));
/// assert_eq!(event.event_type, "user_clicked");
/// assert!(event.ts > 0);
///
/// // With metadata
/// let event = Event::new("order_placed", json!({"total": 99.99}))
///     .with_id("ord-001")
///     .with_actor("user_42");
/// assert_eq!(event.id, Some("ord-001".to_string()));
/// assert_eq!(event.actor, Some("user_42".to_string()));
/// ```
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[non_exhaustive]
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

    /// Unique event identifier.
    ///
    /// Not auto-generated — callers provide their own (uuid, ulid, etc.)
    /// or leave as `None` for simple use cases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Identity of the actor that caused this event (user ID, service name,
    /// API key, etc.). Interpretation is application-defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,

    /// Extensible metadata bag for cross-cutting concerns.
    ///
    /// Typical keys: `"session"`, `"correlation_id"`, `"source"`,
    /// `"schema_version"`. Kept separate from `data` so domain payloads
    /// stay clean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}

impl Event {
    /// Create a new event with the given type and data.
    ///
    /// The timestamp is set to the current time (seconds since Unix epoch).
    /// Metadata fields (`id`, `actor`, `meta`) default to `None` — use the
    /// builder methods to set them.
    ///
    /// # Panics
    ///
    /// Panics if the system clock is set before the Unix epoch.
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
    /// assert_eq!(event.id, None);
    /// assert_eq!(event.actor, None);
    /// assert_eq!(event.meta, None);
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
            id: None,
            actor: None,
            meta: None,
        }
    }

    /// Set the event's unique identifier.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventfold::Event;
    /// use serde_json::json;
    ///
    /// let event = Event::new("click", json!({})).with_id("evt-123");
    /// assert_eq!(event.id, Some("evt-123".to_string()));
    /// ```
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the actor that caused this event.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventfold::Event;
    /// use serde_json::json;
    ///
    /// let event = Event::new("click", json!({})).with_actor("user_42");
    /// assert_eq!(event.actor, Some("user_42".to_string()));
    /// ```
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// Set extensible metadata.
    ///
    /// # Examples
    ///
    /// ```
    /// use eventfold::Event;
    /// use serde_json::json;
    ///
    /// let event = Event::new("click", json!({}))
    ///     .with_meta(json!({"session": "sess_abc"}));
    /// assert!(event.meta.is_some());
    /// ```
    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = Some(meta);
        self
    }
}
