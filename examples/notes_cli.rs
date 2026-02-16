//! Tagged notes with search — a richer CLI app with two views.
//!
//! Demonstrates multiple views over richer event data: a notes view
//! for the note list, and a tags view for tag frequency statistics.

use eventfold::{Event, EventLog};
use serde::{Deserialize, Serialize};
use serde_json::json;
use std::collections::HashMap;

// --- Notes view ---

#[derive(Default, Clone, Serialize, Deserialize)]
struct NotesState {
    notes: Vec<Note>,
    next_id: u64,
}

#[derive(Clone, Serialize, Deserialize)]
struct Note {
    id: u64,
    text: String,
    tags: Vec<String>,
}

fn notes_reducer(mut state: NotesState, event: &Event) -> NotesState {
    if event.event_type == "note_added" {
        let text = event.data["text"].as_str().unwrap_or("").to_string();
        let tags: Vec<String> = event.data["tags"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|v| v.as_str().map(String::from))
                    .collect()
            })
            .unwrap_or_default();
        state.notes.push(Note {
            id: state.next_id,
            text,
            tags,
        });
        state.next_id += 1;
    }
    state
}

// --- Tags view ---

#[derive(Default, Clone, Serialize, Deserialize)]
struct TagsState {
    counts: HashMap<String, u64>,
}

fn tags_reducer(mut state: TagsState, event: &Event) -> TagsState {
    if event.event_type == "note_added"
        && let Some(tags) = event.data["tags"].as_array()
    {
        for tag in tags {
            if let Some(t) = tag.as_str() {
                *state.counts.entry(t.to_string()).or_insert(0) += 1;
            }
        }
    }
    state
}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let dir = tempfile::tempdir()?;
    let mut log = EventLog::builder(dir.path())
        .view::<NotesState>("notes", notes_reducer)
        .view::<TagsState>("tags", tags_reducer)
        .open()?;

    // Add notes
    log.append(&Event::new(
        "note_added",
        json!({"text": "Fix login bug", "tags": ["bug", "auth"]}),
    ))?;
    println!("Added note: \"Fix login bug\" [bug, auth]");

    log.append(&Event::new(
        "note_added",
        json!({"text": "Add dark mode", "tags": ["feature", "ui"]}),
    ))?;
    println!("Added note: \"Add dark mode\" [feature, ui]");

    log.append(&Event::new(
        "note_added",
        json!({"text": "Update deps", "tags": ["maintenance"]}),
    ))?;
    println!("Added note: \"Update deps\" [maintenance]");

    // Refresh both views
    log.refresh_all()?;

    // List all notes
    let notes: &NotesState = log.view("notes")?;
    println!("\nAll notes ({}):", notes.notes.len());
    for note in &notes.notes {
        println!(
            "  {}. {} [{}]",
            note.id + 1,
            note.text,
            note.tags.join(", ")
        );
    }

    // Filter by tag
    let bug_notes: Vec<_> = notes.notes.iter().filter(|n| n.tags.contains(&"bug".to_string())).collect();
    println!("\nNotes tagged 'bug' ({}):", bug_notes.len());
    for note in bug_notes {
        println!(
            "  {}. {} [{}]",
            note.id + 1,
            note.text,
            note.tags.join(", ")
        );
    }

    // Tag stats
    let tags: &TagsState = log.view("tags")?;
    println!("\nTag stats:");
    let mut sorted_tags: Vec<_> = tags.counts.iter().collect();
    sorted_tags.sort_by_key(|(k, _)| (*k).clone());
    for (tag, count) in sorted_tags {
        println!("  {}: {}", tag, count);
    }

    Ok(())
}
