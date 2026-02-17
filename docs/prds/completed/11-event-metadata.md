> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 11: Event Metadata

## Summary

Extend the `Event` struct with optional, structured metadata fields — `id`, `actor`, and `meta` — to support multi-user applications, audit trails, event correlation, and schema evolution. All new fields are optional and backward-compatible with existing logs.

## Prerequisites

- PRD 01–10 (all prior components)

## Motivation

The current `Event` carries only `event_type`, `data`, and `ts`. This is sufficient for single-user tools, but multi-user webapps need answers to questions the current shape can't provide:

- **Who did it?** No actor identity — reducers must dig through `data` ad-hoc.
- **Which event is this?** No stable identifier — events can't be referenced, deduplicated, or correlated across systems.
- **What else do we know?** No place for cross-cutting concerns (session, source, correlation ID, schema version) without polluting the domain `data`.

Stuffing this into `data` works but scatters infrastructure concerns across every event type and every reducer. First-class metadata fields give reducers clean access to provenance without coupling domain logic to envelope concerns.

## Scope

**In scope:**
- Add `id: Option<String>` to `Event` — unique event identifier
- Add `actor: Option<String>` to `Event` — who caused this event
- Add `meta: Option<Value>` to `Event` — extensible metadata bag for application-specific concerns (session ID, correlation ID, schema version, source system, etc.)
- Update `Event::new()` — unchanged signature, new fields default to `None`
- Add `Event::with_id()`, `Event::with_actor()`, `Event::with_meta()` — builder-style setters that return `Self`
- Backward compatibility: existing `.jsonl` files deserialize correctly (missing fields become `None`)
- Forward compatibility: events with metadata serialize cleanly, old consumers ignore unknown fields

**Out of scope:**
- Auto-generating `id` (callers choose their own ID scheme — uuid, ulid, nanoid, etc.)
- Enforcing uniqueness of `id` at the log level
- Authentication or authorization (actor is a label, not a security boundary)
- Sequence numbers at the log level (this is an event-level concern, not log-level; could live in `meta` if needed)
- Changes to `ReduceFn` signature or `ViewOps` trait

## Types

```rust
// src/event.rs

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Event {
    #[serde(rename = "type")]
    pub event_type: String,
    pub data: Value,
    pub ts: u64,

    /// Unique event identifier. Not auto-generated — callers provide their own
    /// (uuid, ulid, etc.) or leave as `None` for simple use cases.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,

    /// Identity of the actor that caused this event (user ID, service name,
    /// API key, etc.). Interpretation is application-defined.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub actor: Option<String>,

    /// Extensible metadata bag for cross-cutting concerns. Typical keys:
    /// `"session"`, `"correlation_id"`, `"source"`, `"schema_version"`.
    /// Kept separate from `data` so domain payloads stay clean.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub meta: Option<Value>,
}
```

### Builder-Style Setters

```rust
impl Event {
    pub fn new(event_type: &str, data: Value) -> Self {
        // unchanged — id, actor, meta all None
    }

    /// Set the event's unique identifier.
    pub fn with_id(mut self, id: impl Into<String>) -> Self {
        self.id = Some(id.into());
        self
    }

    /// Set the actor that caused this event.
    pub fn with_actor(mut self, actor: impl Into<String>) -> Self {
        self.actor = Some(actor.into());
        self
    }

    /// Set extensible metadata.
    pub fn with_meta(mut self, meta: Value) -> Self {
        self.meta = Some(meta);
        self
    }
}
```

### Usage Examples

**Simple (unchanged):**
```rust
let event = Event::new("page_view", json!({"url": "/home"}));
// {"type":"page_view","data":{"url":"/home"},"ts":1739700000}
```

**Multi-user webapp:**
```rust
let event = Event::new("todo_added", json!({"text": "buy milk"}))
    .with_id(Uuid::new_v4().to_string())
    .with_actor("user_42".to_string());
// {"type":"todo_added","data":{"text":"buy milk"},"ts":1739700000,"id":"550e...","actor":"user_42"}
```

**With metadata:**
```rust
let event = Event::new("order_placed", json!({"total": 99.99}))
    .with_id("ord-001")
    .with_actor("user_42")
    .with_meta(json!({
        "session": "sess_abc",
        "correlation_id": "req_xyz",
        "schema_version": 2
    }));
```

**Reducer consuming metadata:**
```rust
fn audit_reducer(mut state: AuditLog, event: &Event) -> AuditLog {
    state.entries.push(AuditEntry {
        event_type: event.event_type.clone(),
        actor: event.actor.clone().unwrap_or_default(),
        timestamp: event.ts,
    });
    state
}
```

## Serialization Details

### JSON Field Order

Serialized output places new fields after the existing three, preserving the familiar shape for simple events:

```json
{"type":"todo_added","data":{"text":"hi"},"ts":1739700000}
{"type":"todo_added","data":{"text":"hi"},"ts":1739700000,"id":"abc","actor":"user_1"}
{"type":"todo_added","data":{"text":"hi"},"ts":1739700000,"id":"abc","actor":"user_1","meta":{"session":"s1"}}
```

### Backward Compatibility

- `#[serde(default)]` on new fields means existing `.jsonl` files (without `id`, `actor`, `meta`) deserialize without error — missing fields become `None`.
- `#[serde(skip_serializing_if = "Option::is_none")]` means simple events don't emit the new fields, keeping output identical to the current format.
- No migration needed. Old logs just work.

### Hash Stability

Line hashes (xxh64) are computed on raw serialized bytes. Adding metadata fields changes the serialized bytes and therefore the hash. This is correct — a different event should have a different hash. Existing snapshots remain valid because they reference events that were serialized without metadata (and those bytes haven't changed).

## Implementation Details

### Struct Literal Construction

Tests and `dummy_event()` construct `Event` with struct literals. Adding new fields to the struct will cause compilation errors in every struct literal that doesn't include them. Fix by adding the new fields:

```rust
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
```

All existing test files that construct `Event { ... }` directly must be updated similarly. This is a mechanical change — search for `Event {` across the codebase.

### No Changes to Reducers or Views

`ReduceFn<S>` remains `fn(S, &Event) -> S`. Reducers that don't care about metadata simply ignore the new fields (they already ignore `ts` in many cases). Reducers that want metadata access it directly: `event.actor`, `event.id`, `event.meta`.

### No Changes to Log I/O

`append()`, `read_from()`, `read_full()` — all work on serialized JSON lines. The serialization change is transparent to them.

## Files

| File | Action |
|------|--------|
| `src/event.rs` | Update — add `id`, `actor`, `meta` fields and builder methods |
| `tests/common/mod.rs` | Update — add new fields to `dummy_event()` struct literal |
| `tests/event_tests.rs` | Update — add new fields to struct literals, add new test cases |
| `tests/log_tests.rs` | Update — add new fields to any `Event { ... }` literals |
| `tests/props.rs` | Update — add new fields to any `Event { ... }` literals |

## Acceptance Criteria

1. **Backward compatible deserialization:** Existing JSON lines without `id`, `actor`, `meta` deserialize to `Event` with all new fields as `None`
2. **Skip serialization when None:** `Event::new("x", json!({}))` serializes identically to the current format (no `id`, `actor`, or `meta` keys in output)
3. **Round-trip with metadata:** Event with all fields set serializes and deserializes back to an equal value
4. **Round-trip without metadata:** Event with no metadata set serializes and deserializes back to an equal value
5. **Builder methods:** `with_id`, `with_actor`, `with_meta` set the corresponding fields and return `Self`
6. **Builder chaining:** `Event::new(...).with_id(...).with_actor(...).with_meta(...)` works fluently
7. **Mixed log compatibility:** A log containing both old-style events (no metadata) and new-style events (with metadata) can be read and reduced correctly
8. **Partial metadata:** Events with only some metadata fields set (e.g., `id` but no `actor`) serialize and deserialize correctly
9. **Reducer access:** A reducer can read `event.actor` and `event.id` to make decisions
10. **Existing tests pass:** All existing tests continue to pass after updating struct literals
11. **Cargo builds and all tests pass**

## Test Plan

### Updated Test Helpers (`tests/common/mod.rs`)

```rust
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

pub fn dummy_event_with_actor(event_type: &str, actor: &str) -> Event {
    Event::new(event_type, json!({"key": "value"}))
        .with_actor(actor)
}
```

### New Test Cases (`tests/event_tests.rs`)

- `test_new_fields_default_none` — `Event::new(...)` has `id`, `actor`, `meta` all `None`
- `test_with_id` — `Event::new(...).with_id("abc")` sets `id` to `Some("abc")`
- `test_with_actor` — `Event::new(...).with_actor("user_1")` sets `actor` to `Some("user_1")`
- `test_with_meta` — `Event::new(...).with_meta(json!({...}))` sets `meta`
- `test_builder_chaining` — all three builder methods chained, all fields set
- `test_serialize_without_metadata` — no `id`/`actor`/`meta` keys in JSON output when `None`
- `test_serialize_with_metadata` — all keys present in JSON output when `Some`
- `test_serialize_partial_metadata` — only set fields appear in output
- `test_deserialize_legacy_format` — JSON without new fields deserializes, new fields are `None`
- `test_deserialize_with_metadata` — JSON with new fields deserializes correctly
- `test_round_trip_with_metadata` — serialize then deserialize with all fields, assert equality
- `test_mixed_log_events` — serialize old-style and new-style events to lines, deserialize all, verify

### Integration Test (`tests/metadata_integration_tests.rs`)

- `test_append_and_read_with_metadata` — append events with actor/id, read back, verify fields preserved
- `test_reducer_reads_actor` — reducer that branches on `event.actor`, verify correct state
- `test_view_with_mixed_events` — view processes old-style and new-style events in same log
- `test_rotation_preserves_metadata` — append with metadata, rotate, rebuild from archive, verify metadata intact
