# Todo App — eventfold + Leptos

A complete web application that uses **eventfold** as its entire data layer. No database, no ORM — just an append-only event log with derived views, served by Leptos SSR.

## What This Demonstrates

- eventfold powering a real web app's state management
- Two derived views (todos + statistics) computed from the same event stream
- Server functions wrapping synchronous eventfold behind `Arc<Mutex<>>`
- Data that persists across server restarts as plain JSON files
- Full CRUD: add, toggle complete, and delete todos

## Prerequisites

- **Rust** (stable, 1.85+)
- **cargo-leptos** — install with `cargo install cargo-leptos`
- **wasm32 target** — add with `rustup target add wasm32-unknown-unknown`

## How to Run

```bash
cd examples-leptos/todo-app
cargo leptos watch
```

Open [http://127.0.0.1:3000](http://127.0.0.1:3000) in your browser.

To verify the example compiles without cargo-leptos:

```bash
cargo build --features ssr
```

## What to Look at First

1. **`src/state.rs`** — The data model. Pure Rust types and reducer functions, no framework dependency. This is the core of the event-sourced design.
2. **`src/server.rs`** — How eventfold integrates with Leptos server functions. Each function locks the shared `EventLog`, appends events or reads views, and returns.
3. **`src/main.rs`** — Server setup. Initializes the `EventLog` with two views and provides it to Leptos via context.

## Where the Data Lives

All data is stored in `./data/` relative to where you run the server:

```
data/
  app.jsonl              # Active event log (one JSON event per line)
  archive.jsonl.zst      # Compressed older events (after rotation)
  views/
    todos.snapshot.json   # Materialized todo list state
    stats.snapshot.json   # Materialized statistics state
```

## How to Inspect the Data

```bash
# View all events
cat data/app.jsonl | jq .

# View the current todo state
cat data/views/todos.snapshot.json | jq .state

# View statistics
cat data/views/stats.snapshot.json | jq .state
```

## Deliberate Constraints

This example is intentionally simple:

- **Single process** — The `EventLog` is wrapped in `Arc<Mutex<>>`, which works for one server process. This is not designed for multi-server deployments.
- **Synchronous I/O** — eventfold operations are synchronous behind a mutex. The lock is held briefly (microseconds for typical operations), so this is fine for the target use case.
- **No authentication** — Anyone who can reach the server can modify todos.

These constraints are appropriate for personal tools, prototypes, internal dashboards, and small team applications — exactly the use cases eventfold is designed for.

## Dependencies

| Crate | Purpose |
|-------|---------|
| `eventfold` | Append-only event log with derived views |
| `leptos` | Reactive web framework (SSR + hydration) |
| `leptos_axum` | Axum integration for Leptos |
| `axum` | HTTP server framework |
| `tokio` | Async runtime |
| `uuid` | Generating unique todo IDs |
| `serde` / `serde_json` | Serialization for state types and event data |
