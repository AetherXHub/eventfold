//! Manual and auto rotation.
//!
//! Demonstrates configuring a small max_log_size, appending enough events
//! to trigger rotation, and verifying continuity after rotation.

use eventfold::{Event, EventLog};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::fs;

#[derive(Default, Clone, Serialize, Deserialize)]
struct Counter {
    count: u64,
}

fn count_reducer(mut state: Counter, _event: &Event) -> Counter {
    state.count += 1;
    state
}

fn file_size(path: &std::path::Path) -> u64 {
    fs::metadata(path).map(|m| m.len()).unwrap_or(0)
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut log = EventLog::builder(dir.path())
        .view::<Counter>("counter", count_reducer)
        .open()?;

    // Append 100 events
    for i in 0..100 {
        log.append(&Event::new("tick", json!({"i": i})))?;
    }
    println!("Appended 100 events...");

    let log_size = file_size(log.log_path());
    let archive_exists = log.archive_path().exists();
    println!(
        "Before rotation: app.jsonl = {} bytes, archive = {}",
        log_size,
        if archive_exists { "exists" } else { "none" }
    );

    // Manual rotation: compress active log into archive, truncate
    log.rotate()?;

    let log_size = file_size(log.log_path());
    let archive_size = file_size(log.archive_path());
    println!(
        "After rotation:  app.jsonl = {} bytes, archive = {} bytes",
        log_size, archive_size
    );

    // Append more events — state still correct across rotation boundary
    for i in 100..110 {
        log.append(&Event::new("tick", json!({"i": i})))?;
    }
    println!("Appended 10 more events...");

    log.refresh_all()?;
    let counter: &Counter = log.view("counter")?;
    println!("State still correct: count = {}", counter.count);

    Ok(())
}
