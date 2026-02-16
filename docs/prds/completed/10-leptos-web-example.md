> **Status: COMPLETED** — Implemented and verified on 2026-02-16

# PRD 10: Leptos Web Application Example

## Summary

Create a complete, working Leptos SSR web application that uses eventfold as its entire data layer. This is the flagship example — it demonstrates that eventfold can power a real web app, not just CLI scripts. The example lives in a separate crate (`examples-leptos/todo-app/`) with its own `Cargo.toml`.

## Prerequisites

- PRD 01–09 (full system implemented, tested, and documented)

## Scope

**In scope:**
- A standalone Leptos SSR todo application in `examples-leptos/todo-app/`
- Server functions wrapping eventfold operations (`Arc<Mutex<EventLog>>`)
- Two views: todo state and statistics
- Full CRUD: add, toggle, delete todos
- Live statistics sidebar
- A README for the example explaining setup, architecture, and data inspection

**Out of scope:**
- Publishing the example as a crate
- Authentication or authorization
- Client-side optimistic UI (keep it simple — full page reactivity via server functions)
- Styling beyond basic functional CSS (no Tailwind, no component library)
- Deployment configuration

## Architecture

```
Browser (Leptos client)
  ↕ server functions (HTTP)
Leptos server (Axum)
  ↕ eventfold API (sync, behind Arc<Mutex<>>)
data/
  archive.jsonl.zst
  app.jsonl
  views/
    todos.snapshot.json
    stats.snapshot.json
```

The core `eventfold` crate remains synchronous. The Leptos example wraps it in `Arc<Mutex<EventLog>>` and calls it from async server functions. The lock is held only briefly during append/refresh — fine for the target use case.

## Dependencies

The example is a **separate crate** with its own `Cargo.toml`:

| Crate | Purpose |
|-------|---------|
| `eventfold` | Path dependency to parent crate (`{ path = "../.." }`) |
| `leptos` | Reactive web framework (SSR features) |
| `leptos_axum` | Axum server integration for Leptos |
| `axum` | HTTP framework |
| `tokio` | Async runtime (required by Leptos/Axum) |
| `uuid` | Generating unique todo IDs |
| `serde` | Shared serialization (derive) |
| `serde_json` | JSON data construction |
| `tower-http` | Serving static files (if needed) |

Use the latest stable versions of all dependencies. Leptos 0.7+ with Axum integration.

## Files

| File | Action |
|------|--------|
| `examples-leptos/todo-app/Cargo.toml` | Create |
| `examples-leptos/todo-app/src/main.rs` | Create |
| `examples-leptos/todo-app/src/app.rs` | Create |
| `examples-leptos/todo-app/src/state.rs` | Create |
| `examples-leptos/todo-app/src/server.rs` | Create |
| `examples-leptos/todo-app/src/components/todo_list.rs` | Create |
| `examples-leptos/todo-app/src/components/todo_item.rs` | Create |
| `examples-leptos/todo-app/src/components/stats.rs` | Create |
| `examples-leptos/todo-app/src/components/mod.rs` | Create |
| `examples-leptos/todo-app/README.md` | Create |

## Implementation

### `state.rs` — Event types, reducers, view definitions

Define the event vocabulary, state types, and reducer functions. These are pure Rust — no Leptos dependency.

**Events:**
```
"todo_added"    { "id": "<uuid>", "text": "...", "created_at": <unix_ts> }
"todo_toggled"  { "id": "<uuid>" }
"todo_deleted"  { "id": "<uuid>" }
```

**State types:**

```rust
#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct TodoState {
    pub items: Vec<Todo>,
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Todo {
    pub id: String,
    pub text: String,
    pub done: bool,
    pub created_at: u64,
}

#[derive(Default, Clone, Debug, Serialize, Deserialize)]
pub struct StatsState {
    pub total_created: u64,
    pub total_completed: u64,
    pub total_deleted: u64,
}
```

**Reducers:**

```rust
pub fn todo_reducer(mut state: TodoState, event: &Event) -> TodoState {
    match event.event_type.as_str() {
        "todo_added" => {
            state.items.push(Todo {
                id: event.data["id"].as_str().unwrap_or("").to_string(),
                text: event.data["text"].as_str().unwrap_or("").to_string(),
                done: false,
                created_at: event.data["created_at"].as_u64().unwrap_or(0),
            });
        }
        "todo_toggled" => {
            let id = event.data["id"].as_str().unwrap_or("");
            if let Some(item) = state.items.iter_mut().find(|i| i.id == id) {
                item.done = !item.done;
            }
        }
        "todo_deleted" => {
            let id = event.data["id"].as_str().unwrap_or("");
            state.items.retain(|i| i.id != id);
        }
        _ => {}
    }
    state
}

pub fn stats_reducer(mut state: StatsState, event: &Event) -> StatsState {
    match event.event_type.as_str() {
        "todo_added" => state.total_created += 1,
        "todo_toggled" => state.total_completed += 1,
        "todo_deleted" => state.total_deleted += 1,
        _ => {}
    }
    state
}
```

### `server.rs` — Server functions wrapping eventfold

Each server function acquires the `Arc<Mutex<EventLog>>` from Leptos server context, locks it, performs the eventfold operation, and releases.

**Functions:**

- `get_todos() -> Result<TodoState, ServerFnError>` — refresh, return cloned TodoState
- `get_stats() -> Result<StatsState, ServerFnError>` — refresh, return cloned StatsState
- `add_todo(text: String) -> Result<(), ServerFnError>` — generate UUID, append `todo_added` event, refresh
- `toggle_todo(id: String) -> Result<(), ServerFnError>` — append `todo_toggled` event, refresh
- `delete_todo(id: String) -> Result<(), ServerFnError>` — append `todo_deleted` event, refresh

Each mutating function calls `refresh_all()` after appending so that the next `get_todos()` or `get_stats()` returns current state.

**Extracting the log from context:**

```rust
fn use_eventfold() -> Result<Arc<Mutex<EventLog>>, ServerFnError> {
    use_context::<Arc<Mutex<EventLog>>>()
        .ok_or_else(|| ServerFnError::new("EventLog not found in server context"))
}
```

### `main.rs` — Server setup

Initialize eventfold, wrap in `Arc<Mutex<>>`, provide to Leptos server context, start Axum server.

```rust
#[tokio::main]
async fn main() {
    let log = EventLog::builder("./data")
        .max_log_size(10_000_000)
        .view::<TodoState>("todos", todo_reducer)
        .view::<StatsState>("stats", stats_reducer)
        .open()
        .expect("failed to open event log");

    let log = Arc::new(Mutex::new(log));

    // Standard Leptos + Axum SSR setup
    // Provide log to server context
    // Serve on 0.0.0.0:3000
}
```

### `app.rs` — Root Leptos component

Sets up the Leptos app shell with a single route (`/`) rendering the todo list and stats sidebar. Minimal layout — a header, main content area with the todo list, and a stats section.

### Components

#### `components/todo_list.rs`

- Fetches todos via `get_todos` server function using a Leptos resource
- Renders an input form that calls `add_todo` via a server action
- Lists all todos using `TodoItem` component
- Refetches the todo list after any mutation (add, toggle, delete)

#### `components/todo_item.rs`

- Renders a single todo row: checkbox (toggle), text, delete button
- Toggle checkbox calls `toggle_todo` server function
- Delete button calls `delete_todo` server function
- Visual distinction for completed items (strikethrough or similar)

#### `components/stats.rs`

- Fetches stats via `get_stats` server function
- Displays: total created, total completed, total deleted
- Computes and shows completion rate

### `README.md`

The example README should explain:

1. **What this demonstrates** — eventfold as a web app's entire data layer, no database
2. **How to run it** — prerequisites (Rust, cargo-leptos), then `cargo leptos watch`
3. **What to look at first** — `state.rs` for the data model, `server.rs` for the integration
4. **Where the data lives** — `./data/` directory
5. **How to inspect state** — `cat data/app.jsonl | jq .`, snapshot inspection
6. **Deliberate constraints** — single-process, no multi-server deployment, and why that's fine for personal tools, prototypes, and small teams
7. **Dependencies** — what each crate provides

## Acceptance Criteria

1. **The example compiles:** `cargo build` in `examples-leptos/todo-app/` succeeds
2. **The example runs:** `cargo leptos watch` (or `cargo run` if not using cargo-leptos) starts the server and serves the app
3. **CRUD works:** Can add, toggle, and delete todos through the web UI
4. **Stats update:** Statistics view reflects changes after each action
5. **Data persists:** Stopping and restarting the server preserves all todos
6. **Data inspectable:** `cat data/app.jsonl | jq .` shows events, snapshot files are readable
7. **README is complete:** All 7 sections listed above are present
8. **No changes to core eventfold crate:** The example is entirely self-contained in `examples-leptos/`
9. **Clippy clean:** `cargo clippy -- -D warnings` in the example directory produces no warnings
