#![warn(missing_docs)]

//! # eventfold
//!
//! Your application state is a fold over an event log.
//!
//! eventfold is a lightweight, append-only event log with derived views for Rust.
//! You define events as JSON, write pure reducer functions to fold them into state,
//! and let the library handle persistence, snapshots, and log rotation. No database,
//! no infrastructure — just files in a directory.
//!
//! ## Quick Start
//!
//! ```
//! # use tempfile::tempdir;
//! use eventfold::{Event, EventLog};
//! use serde::{Serialize, Deserialize};
//! use serde_json::json;
//!
//! #[derive(Default, Clone, Serialize, Deserialize)]
//! struct Counter { count: u64 }
//!
//! fn count_reducer(mut state: Counter, _event: &Event) -> Counter {
//!     state.count += 1;
//!     state
//! }
//!
//! # let dir = tempdir().unwrap();
//! let mut log = EventLog::builder(dir.path())
//!     .view::<Counter>("counter", count_reducer)
//!     .open()
//!     .unwrap();
//!
//! log.append(&Event::new("click", json!({"x": 10}))).unwrap();
//! log.refresh_all().unwrap();
//!
//! let state: &Counter = log.view("counter").unwrap();
//! assert_eq!(state.count, 1);
//! ```
//!
//! ## Core Concepts
//!
//! - **Events** are immutable JSON records appended to a log file (`app.jsonl`).
//! - **Reducers** are pure functions `fn(State, &Event) -> State` that fold events
//!   into application state.
//! - **Views** are derived state computed by applying a reducer to the event log,
//!   with snapshots on disk for incremental performance.
//!
//! See `docs/guide.md` for a detailed concepts guide.

mod archive;
mod event;
mod log;
pub mod snapshot;
mod view;

pub use event::Event;
pub use log::{
    line_hash, AppendConflict, AppendResult, ConditionalAppendError, EventLog, EventLogBuilder,
    EventReader, EventWriter, LockMode, WaitResult,
};
pub use snapshot::Snapshot;
pub use view::{ReduceFn, View, ViewOps};
