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
        "float": 3.14,
        "zero": 0,
        "large": 9999999999u64
    });
    let event = Event {
        event_type: "numbers".to_string(),
        data: data.clone(),
        ts: 1000,
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
