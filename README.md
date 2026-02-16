# eventfold

Your application state is a fold over an event log.

**eventfold** is a lightweight, append-only event log with derived views for Rust. Your application state is always a function of the log — computed by folding events through pure reducer functions. Snapshots cache the result for incremental performance. Zero infrastructure: just files in a directory.

```
state = events.reduce(reducer, initial_state)
```

## Quick Example

```rust
use eventfold::{EventLog, Event, ReduceFn};
use serde::{Serialize, Deserialize};
use serde_json::json;

#[derive(Default, Clone, Serialize, Deserialize, Debug)]
struct TodoState {
    items: Vec<TodoItem>,
    next_id: u64,
}

#[derive(Clone, Serialize, Deserialize, Debug)]
struct TodoItem {
    id: u64,
    text: String,
    done: bool,
}

fn todo_reducer(mut state: TodoState, event: &Event) -> TodoState {
    match event.event_type.as_str() {
        "todo_added" => {
            state.items.push(TodoItem {
                id: state.next_id,
                text: event.data["text"].as_str().unwrap().to_string(),
                done: false,
            });
            state.next_id += 1;
        }
        "todo_completed" => {
            let id = event.data["id"].as_u64().unwrap();
            if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                item.done = true;
            }
        }
        "todo_deleted" => {
            let id = event.data["id"].as_u64().unwrap();
            state.items.retain(|i| i.id != id);
        }
        _ => {}
    }
    state
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut app = EventLog::builder("./data")
        .max_log_size(10_000_000)
        .view::<TodoState>("todos", todo_reducer)
        .open()?;

    app.append(&Event::new("todo_added", json!({"text": "buy milk"})))?;
    app.append(&Event::new("todo_added", json!({"text": "write docs"})))?;
    app.refresh_all()?;

    let todos: &TodoState = app.view("todos")?;
    println!("{:?}", todos);
    Ok(())
}
```

This is the entire data layer. No schema, no migrations, no ORM. The log file is the database. The reducer is the schema.

## Core Concepts

**Events** are append-only JSON lines in a log file. Each event has a type, arbitrary JSON data, and a timestamp. The log never rewrites or deletes events.

**Reducers** are pure functions `fn(State, &Event) -> State` that give events meaning. They fold events into application state. Different reducers over the same log produce different views — same data, different lenses.

**Views** are derived state materialized by folding events through a reducer. Snapshots cache the result so subsequent refreshes only process new events. A view is always rebuildable from the full log.

## Installation

```
cargo add eventfold
```

## Features

- Append-only event log (JSONL)
- Derived views via pure reducer functions
- Incremental snapshots — only process new events on refresh
- Automatic log rotation with zstd compression
- Integrity checking via xxhash — auto-rebuild on corruption
- Crash-safe — atomic snapshot writes, graceful recovery from partial writes
- Zero infrastructure — just files in a directory
- Single-crate, minimal dependencies, no async

## Data Layout

```
data/
  archive.jsonl.zst          # compressed event history (zstd frames)
  app.jsonl                  # active log, plain text, append-only
  views/
    todos.snapshot.json      # {"state": {...}, "offset": 12840, "hash": "a3f2..."}
    stats.snapshot.json
```

Only two data files: the compressed archive and the active log. Views are cached snapshots that are always rebuildable.

## When to Use

- Personal tools, CLIs, small web apps
- Prototypes and MVPs where you want persistence without a database
- Applications where the event history is valuable (audit logs, undo, time travel)
- Embedded state in single-process applications
- Any case where "just files in a directory" is the right level of infrastructure

## When Not to Use

- High-concurrency or multi-process writers
- Distributed systems
- High write throughput (every append flushes to disk)
- Applications needing ad-hoc queries or indexes beyond what reducers build
- Anything requiring encryption or access control at the storage layer

## Examples

```
cargo run --example todo_cli       # minimal CLI todo app
cargo run --example multi_view     # same log, multiple views
cargo run --example rebuild        # changing a reducer and rebuilding
cargo run --example rotation       # manual and auto rotation
cargo run --example time_travel    # replaying to a specific point
cargo run --example notes_cli      # tagged notes with search
```

## Documentation

- [Concepts & Guide](docs/guide.md) — how it works, writing reducers, schema evolution, crash safety, debugging
- [API Reference](https://docs.rs/eventfold) — rustdoc for all public types and methods

## License

MIT OR Apache-2.0
