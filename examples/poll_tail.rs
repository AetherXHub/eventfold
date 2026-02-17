//! Poll-based tailing — catch up on new events with a sleep loop.
//!
//! Spawns a background writer that appends events every 200ms.
//! The main thread polls with `has_new_events` + sleep to detect
//! and process new events as they arrive.

use eventfold::{Event, EventWriter};
use serde_json::json;
use std::thread;
use std::time::Duration;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;

    // Create writer and get a reader before moving the writer to a thread.
    let mut writer = EventWriter::open(dir.path())?;
    let reader = writer.reader();

    // Background writer: append 10 events, one every 200ms.
    let handle = thread::spawn(move || {
        for i in 0..10 {
            thread::sleep(Duration::from_millis(200));
            writer
                .append(&Event::new("tick", json!({"i": i})))
                .unwrap();
            println!("[writer] appended tick {i}");
        }
    });

    // Poll loop: check for new events every 50ms.
    let mut offset = 0u64;
    let mut seen = 0usize;

    while seen < 10 {
        if reader.has_new_events(offset)? {
            for result in reader.read_from(offset)? {
                let (event, next_offset, _hash) = result?;
                let i = event.data["i"].as_u64().unwrap();
                println!("[reader] saw tick {i}");
                offset = next_offset;
                seen += 1;
            }
        } else {
            thread::sleep(Duration::from_millis(50));
        }
    }

    handle.join().unwrap();
    println!("\nDone — processed {seen} events via poll-based tailing.");

    Ok(())
}
