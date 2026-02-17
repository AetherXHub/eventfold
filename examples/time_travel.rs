//! Replaying to a specific point in the event history.
//!
//! Shows how to read events one by one and fold manually, stopping
//! at any point to inspect intermediate state.

use eventfold::{Event, EventLog};
use serde_json::json;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut log = EventLog::open(dir.path())?;

    // Append 20 events
    for i in 0..20 {
        log.append(&Event::new("tick", json!({"i": i})))?;
    }

    // Full state: fold all events
    let full_count = count_events(&log, 20)?;
    println!("Full state (20 events): count = {}", full_count);

    // State at event 10: stop early
    let count_at_10 = count_events(&log, 10)?;
    println!("State at event 10: count = {}", count_at_10);

    // State at event 5: stop even earlier
    let count_at_5 = count_events(&log, 5)?;
    println!("State at event 5: count = {}", count_at_5);

    Ok(())
}

/// Fold events from the full log, stopping after `limit` events.
fn count_events(log: &EventLog, limit: usize) -> Result<u64, Box<dyn std::error::Error>> {
    let mut count = 0u64;
    for result in log.read_full()?.take(limit) {
        let (_event, _hash) = result?;
        count += 1;
    }
    Ok(count)
}
