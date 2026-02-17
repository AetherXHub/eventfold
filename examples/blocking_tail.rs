//! Blocking tail — wait for new events with OS-level file notifications.
//!
//! Spawns a background writer that appends events every 200ms.
//! The main thread uses `wait_for_events` to block until new data
//! appears, avoiding busy-polling entirely.

use eventfold::{Event, EventWriter, WaitResult};
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

    // Blocking tail loop: wait for new events, process them, repeat.
    let mut offset = 0u64;
    let mut seen = 0usize;

    while seen < 10 {
        match reader.wait_for_events(offset, Duration::from_secs(5))? {
            WaitResult::NewData(_new_size) => {
                for result in reader.read_from(offset)? {
                    let (event, next_offset, _hash) = result?;
                    let i = event.data["i"].as_u64().unwrap();
                    println!("[reader] saw tick {i}");
                    offset = next_offset;
                    seen += 1;
                }
            }
            WaitResult::Timeout => {
                println!("[reader] timeout — no new events in 5s");
            }
        }
    }

    handle.join().unwrap();
    println!("\nDone — processed {seen} events via blocking tail.");

    Ok(())
}
