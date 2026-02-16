# PRD 01: Event Type & Serialization

## Summary

Define the core `Event` type and its JSON serialization. This is the atomic unit of data in eventfold — every other component consumes events.

## Prerequisites

None. This is the foundation.

## Scope

**In scope:**
- `Event` struct with `event_type`, `data`, and `ts` fields
- `Event::new(event_type, data)` constructor that auto-populates timestamp
- JSON serialization via serde (single-line, no pretty printing)
- Guarantee: serialized event is always exactly one line (no embedded newlines in output)

**Out of scope:**
- Event validation or schema enforcement
- Log file I/O (PRD 02)
- Hashing (PRD 02)

## Types

```rust
// src/event.rs

use serde::{Serialize, Deserialize};
use serde_json::Value;
use std::time::{SystemTime, UNIX_EPOCH};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
    pub ts: u64,
}

impl Event {
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
```

## Implementation Details

- `ts` is seconds since Unix epoch, auto-populated in `Event::new`
- `data` is `serde_json::Value` — intentionally untyped. The log has no opinion about event shapes.
- The `#[serde(rename = "type")]` keeps JSON output clean: `{"type": "todo_added", "data": {...}, "ts": 1234}`
- Serialization MUST use `serde_json::to_string` (not `to_string_pretty`) to guarantee single-line output

## Dependencies

```toml
[dependencies]
serde = { version = "1", features = ["derive"] }
serde_json = "1"
```

## Files

| File | Action |
|------|--------|
| `src/event.rs` | Create |
| `src/lib.rs` | Create — re-export `Event` |
| `Cargo.toml` | Add serde, serde_json dependencies |
| `src/main.rs` | Remove (this is a library crate) |
| `tests/common/mod.rs` | Create — `dummy_event()` helper |
| `tests/event_tests.rs` | Create |

## Acceptance Criteria

1. **Round-trip:** `Event` serializes to JSON and deserializes back to an equal value
2. **Field preservation:** `event_type`, `data`, and `ts` survive serialization unchanged
3. **Arbitrary data:** Events with nested objects, arrays, nulls, numbers, and strings in `data` all round-trip correctly
4. **Single-line guarantee:** Serialized JSON contains no newline characters, even when `data` contains string values with embedded newlines (those are escaped as `\n` in JSON)
5. **Special characters:** Unicode, escaped quotes, and other special characters in `data` string values round-trip correctly
6. **Missing fields:** Deserializing JSON missing required fields produces a clear error (not a panic)
7. **Constructor:** `Event::new("type", data)` produces an event with a reasonable `ts` value (within a few seconds of now)
8. **Cargo builds and all tests pass**

## Test Plan

```
tests/
  common/
    mod.rs              # dummy_event(event_type: &str) -> Event
  event_tests.rs        # all acceptance criteria above
```

### Test Helpers (`tests/common/mod.rs`)

```rust
use eventfold::Event;
use serde_json::json;

pub fn dummy_event(event_type: &str) -> Event {
    Event {
        event_type: event_type.to_string(),
        data: json!({"key": "value"}),
        ts: 1000,
    }
}
```

### Test Cases (`tests/event_tests.rs`)

- `test_round_trip` — serialize then deserialize, assert equality
- `test_field_preservation` — check each field individually after round-trip
- `test_arbitrary_data` — nested objects, arrays, nulls, numbers
- `test_single_line_output` — assert no `\n` in serialized string
- `test_special_characters` — unicode, escaped quotes in data values
- `test_embedded_newlines_in_data` — string value with `\n` inside, verify JSON output is still one line
- `test_missing_fields_error` — deserialize `{}`, `{"type": "x"}`, etc. — expect errors
- `test_constructor_timestamp` — `Event::new` produces ts within 2 seconds of `now()`
