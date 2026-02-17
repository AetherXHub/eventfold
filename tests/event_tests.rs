mod common;

use common::dummy_event;
use eventfold::Event;
use serde_json::json;
use std::time::{SystemTime, UNIX_EPOCH};

#[test]
fn test_round_trip() {
    let event = dummy_event("test_event");
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(event, deserialized);
}

#[test]
fn test_field_preservation() {
    let event = Event {
        event_type: "my_type".to_string(),
        data: json!({"count": 42, "name": "alice"}),
        ts: 1700000000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();

    assert_eq!(deserialized.event_type, "my_type");
    assert_eq!(deserialized.data["count"], 42);
    assert_eq!(deserialized.data["name"], "alice");
    assert_eq!(deserialized.ts, 1700000000);
}

#[test]
fn test_arbitrary_data_nested_objects() {
    let data = json!({
        "user": {
            "name": "alice",
            "address": {
                "city": "Portland",
                "zip": 97201
            }
        }
    });
    let event = Event {
        event_type: "nested".to_string(),
        data: data.clone(),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, data);
}

#[test]
fn test_arbitrary_data_arrays() {
    let data = json!({
        "tags": ["rust", "event-sourcing", "crate"],
        "scores": [1, 2, 3, 4, 5]
    });
    let event = Event {
        event_type: "arrays".to_string(),
        data: data.clone(),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, data);
}

#[test]
fn test_arbitrary_data_nulls() {
    let data = json!({
        "present": "yes",
        "absent": null,
        "nested": {"also_null": null}
    });
    let event = Event {
        event_type: "nulls".to_string(),
        data: data.clone(),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, data);
}

#[test]
fn test_arbitrary_data_numbers() {
    let data = json!({
        "integer": 42,
        "negative": -7,
        "float": 1.23,
        "zero": 0,
        "large": 9999999999u64
    });
    let event = Event {
        event_type: "numbers".to_string(),
        data: data.clone(),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, data);
}

#[test]
fn test_arbitrary_data_empty_object() {
    let event = Event {
        event_type: "empty".to_string(),
        data: json!({}),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, json!({}));
}

#[test]
fn test_arbitrary_data_string_value() {
    let event = Event {
        event_type: "string_data".to_string(),
        data: json!("just a string"),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, json!("just a string"));
}

#[test]
fn test_single_line_output() {
    let event = dummy_event("test");
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        !json.contains('\n'),
        "serialized JSON must be a single line, got: {json}"
    );
}

#[test]
fn test_single_line_output_with_complex_data() {
    let event = Event {
        event_type: "complex".to_string(),
        data: json!({
            "nested": {"deep": {"deeper": "value"}},
            "array": [1, 2, 3],
            "null_field": null
        }),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        !json.contains('\n'),
        "serialized JSON must be a single line, got: {json}"
    );
}

#[test]
fn test_embedded_newlines_in_data() {
    let event = Event {
        event_type: "multiline".to_string(),
        data: json!({"text": "line one\nline two\nline three"}),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(
        !json.contains('\n'),
        "embedded newlines in data must be escaped in JSON output, got: {json}"
    );

    // Verify the newlines survive round-trip
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(
        deserialized.data["text"].as_str().unwrap(),
        "line one\nline two\nline three"
    );
}

#[test]
fn test_special_characters_unicode() {
    let data = json!({
        "emoji": "Hello 🌍🦀",
        "chinese": "你好世界",
        "japanese": "こんにちは",
        "arabic": "مرحبا"
    });
    let event = Event {
        event_type: "unicode".to_string(),
        data: data.clone(),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, data);
}

#[test]
fn test_special_characters_escaped_quotes() {
    let data = json!({
        "quote": "He said \"hello\" to her",
        "backslash": "path\\to\\file",
        "tab": "col1\tcol2"
    });
    let event = Event {
        event_type: "escapes".to_string(),
        data: data.clone(),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, data);
}

#[test]
fn test_special_characters_mixed() {
    let data = json!({
        "mixed": "Hello 🌍\n\"quoted\"\ttab\\backslash"
    });
    let event = Event {
        event_type: "mixed".to_string(),
        data: data.clone(),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();
    assert!(!json.contains('\n'));
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(deserialized.data, data);
}

#[test]
fn test_missing_fields_empty_object() {
    let result = serde_json::from_str::<Event>("{}");
    assert!(result.is_err(), "empty object should fail to deserialize");
}

#[test]
fn test_missing_fields_only_type() {
    let result = serde_json::from_str::<Event>(r#"{"type": "x"}"#);
    assert!(
        result.is_err(),
        "object with only type should fail to deserialize"
    );
}

#[test]
fn test_missing_fields_no_data() {
    let result = serde_json::from_str::<Event>(r#"{"type": "x", "ts": 1000}"#);
    assert!(
        result.is_err(),
        "object without data should fail to deserialize"
    );
}

#[test]
fn test_missing_fields_no_ts() {
    let result = serde_json::from_str::<Event>(r#"{"type": "x", "data": {}}"#);
    assert!(
        result.is_err(),
        "object without ts should fail to deserialize"
    );
}

#[test]
fn test_missing_fields_no_type() {
    let result = serde_json::from_str::<Event>(r#"{"data": {}, "ts": 1000}"#);
    assert!(
        result.is_err(),
        "object without type should fail to deserialize"
    );
}

#[test]
fn test_constructor_timestamp() {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs();

    let event = Event::new("test", json!({}));

    // Timestamp should be within 2 seconds of now
    assert!(
        event.ts >= now && event.ts <= now + 2,
        "timestamp {} should be within 2 seconds of {}",
        event.ts,
        now
    );
}

#[test]
fn test_constructor_sets_fields() {
    let event = Event::new("my_event", json!({"key": "val"}));
    assert_eq!(event.event_type, "my_event");
    assert_eq!(event.data, json!({"key": "val"}));
    assert!(event.ts > 0);
}

#[test]
fn test_serde_rename_type_field() {
    let event = dummy_event("renamed");
    let json = serde_json::to_string(&event).unwrap();

    // JSON should use "type" not "event_type"
    assert!(json.contains(r#""type":"renamed""#), "JSON should use 'type' as the field name, got: {json}");
    assert!(!json.contains("event_type"), "JSON should not contain 'event_type', got: {json}");
}

#[test]
fn test_deserialize_from_raw_json() {
    let raw = r#"{"type":"manual","data":{"x":1},"ts":9999}"#;
    let event: Event = serde_json::from_str(raw).unwrap();
    assert_eq!(event.event_type, "manual");
    assert_eq!(event.data["x"], 1);
    assert_eq!(event.ts, 9999);
}

#[test]
fn test_dummy_event_helper() {
    let event = dummy_event("helper_test");
    assert_eq!(event.event_type, "helper_test");
    assert_eq!(event.data, json!({"key": "value"}));
    assert_eq!(event.ts, 1000);
}

// --- PRD 11: Event Metadata Tests ---

#[test]
fn test_new_fields_default_none() {
    let event = Event::new("test", json!({}));
    assert_eq!(event.id, None);
    assert_eq!(event.actor, None);
    assert_eq!(event.meta, None);
}

#[test]
fn test_with_id() {
    let event = Event::new("test", json!({})).with_id("abc");
    assert_eq!(event.id, Some("abc".to_string()));
    assert_eq!(event.actor, None);
    assert_eq!(event.meta, None);
}

#[test]
fn test_with_actor() {
    let event = Event::new("test", json!({})).with_actor("user_1");
    assert_eq!(event.id, None);
    assert_eq!(event.actor, Some("user_1".to_string()));
    assert_eq!(event.meta, None);
}

#[test]
fn test_with_meta() {
    let meta = json!({"session": "sess_abc", "schema_version": 2});
    let event = Event::new("test", json!({})).with_meta(meta.clone());
    assert_eq!(event.id, None);
    assert_eq!(event.actor, None);
    assert_eq!(event.meta, Some(meta));
}

#[test]
fn test_metadata_builder_chaining() {
    let event = Event::new("test", json!({"x": 1}))
        .with_id("evt-001")
        .with_actor("user_42")
        .with_meta(json!({"session": "s1"}));

    assert_eq!(event.id, Some("evt-001".to_string()));
    assert_eq!(event.actor, Some("user_42".to_string()));
    assert_eq!(event.meta, Some(json!({"session": "s1"})));
    assert_eq!(event.event_type, "test");
    assert_eq!(event.data, json!({"x": 1}));
}

#[test]
fn test_serialize_without_metadata() {
    let event = Event {
        event_type: "test".to_string(),
        data: json!({"x": 1}),
        ts: 1000,
        id: None,
        actor: None,
        meta: None,
    };
    let json = serde_json::to_string(&event).unwrap();

    // Should not contain id, actor, or meta keys
    assert!(!json.contains("\"id\""), "None id should be omitted: {json}");
    assert!(
        !json.contains("\"actor\""),
        "None actor should be omitted: {json}"
    );
    assert!(
        !json.contains("\"meta\""),
        "None meta should be omitted: {json}"
    );
    // Should still have the existing fields
    assert!(json.contains("\"type\""));
    assert!(json.contains("\"data\""));
    assert!(json.contains("\"ts\""));
}

#[test]
fn test_serialize_with_metadata() {
    let event = Event::new("test", json!({}))
        .with_id("abc")
        .with_actor("user_1")
        .with_meta(json!({"key": "val"}));
    let json = serde_json::to_string(&event).unwrap();

    assert!(json.contains("\"id\":\"abc\""), "id should be present: {json}");
    assert!(
        json.contains("\"actor\":\"user_1\""),
        "actor should be present: {json}"
    );
    assert!(
        json.contains("\"meta\":{\"key\":\"val\"}"),
        "meta should be present: {json}"
    );
}

#[test]
fn test_serialize_partial_metadata() {
    // Only id set
    let event = Event::new("test", json!({})).with_id("abc");
    let json = serde_json::to_string(&event).unwrap();

    assert!(json.contains("\"id\":\"abc\""));
    assert!(!json.contains("\"actor\""));
    assert!(!json.contains("\"meta\""));
}

#[test]
fn test_deserialize_legacy_format() {
    // JSON without id, actor, meta — simulates an old log entry
    let raw = r#"{"type":"old_event","data":{"x":1},"ts":5000}"#;
    let event: Event = serde_json::from_str(raw).unwrap();

    assert_eq!(event.event_type, "old_event");
    assert_eq!(event.data, json!({"x": 1}));
    assert_eq!(event.ts, 5000);
    assert_eq!(event.id, None);
    assert_eq!(event.actor, None);
    assert_eq!(event.meta, None);
}

#[test]
fn test_deserialize_with_metadata() {
    let raw = r#"{"type":"new_event","data":{},"ts":6000,"id":"evt-1","actor":"user_5","meta":{"v":2}}"#;
    let event: Event = serde_json::from_str(raw).unwrap();

    assert_eq!(event.event_type, "new_event");
    assert_eq!(event.ts, 6000);
    assert_eq!(event.id, Some("evt-1".to_string()));
    assert_eq!(event.actor, Some("user_5".to_string()));
    assert_eq!(event.meta, Some(json!({"v": 2})));
}

#[test]
fn test_round_trip_with_metadata() {
    let event = Event::new("test", json!({"data": "here"}))
        .with_id("id-123")
        .with_actor("actor-456")
        .with_meta(json!({"session": "s", "version": 3}));

    let json = serde_json::to_string(&event).unwrap();
    let deserialized: Event = serde_json::from_str(&json).unwrap();
    assert_eq!(event, deserialized);
}

#[test]
fn test_mixed_log_events() {
    // Simulate a log with old-style and new-style events
    let lines = vec![
        r#"{"type":"old","data":{"v":1},"ts":1000}"#,
        r#"{"type":"new","data":{"v":2},"ts":2000,"id":"e1","actor":"u1"}"#,
        r#"{"type":"partial_meta","data":{"v":3},"ts":3000,"actor":"u2"}"#,
    ];

    let events: Vec<Event> = lines
        .iter()
        .map(|line| serde_json::from_str(line).unwrap())
        .collect();

    assert_eq!(events[0].id, None);
    assert_eq!(events[0].actor, None);

    assert_eq!(events[1].id, Some("e1".to_string()));
    assert_eq!(events[1].actor, Some("u1".to_string()));

    assert_eq!(events[2].id, None);
    assert_eq!(events[2].actor, Some("u2".to_string()));
}
