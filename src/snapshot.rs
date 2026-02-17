//! Snapshot persistence for derived view state.

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

/// A persisted checkpoint of a view's state.
///
/// Snapshots are written atomically to disk (via a `.tmp` + rename) as a side
/// effect of [`View::refresh`](crate::View::refresh). They enable incremental
/// reads — on the next refresh, only events after `offset` need to be processed.
///
/// The snapshot file is JSON and can be inspected directly:
///
/// ```text
/// $ cat views/todos.snapshot.json | jq .
/// {
///   "state": { "items": [...], "next_id": 3 },
///   "offset": 1284,
///   "hash": "a3f2e1b09c4d..."
/// }
/// ```
#[derive(Debug, Clone, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Snapshot<S> {
    /// The derived state at the time of the snapshot.
    pub state: S,

    /// Byte offset into `app.jsonl` after the last event consumed.
    /// Always refers to the active log — any snapshot that exists has already
    /// consumed everything in the archive.
    pub offset: u64,

    /// Hex-encoded xxh64 hash of the last event line processed.
    /// Used for integrity verification on the next refresh.
    pub hash: String,
}

impl<S> Snapshot<S> {
    /// Create a new snapshot.
    pub fn new(state: S, offset: u64, hash: String) -> Self {
        Snapshot {
            state,
            offset,
            hash,
        }
    }
}

/// Save a snapshot atomically to disk.
///
/// Writes to a `.tmp` file first, syncs, then renames to the final path.
/// If the process crashes mid-write, the old snapshot file survives intact.
pub fn save<S: Serialize>(path: &Path, snapshot: &Snapshot<S>) -> io::Result<()> {
    let tmp_path = path.with_extension("json.tmp");

    let json = serde_json::to_string_pretty(snapshot)
        .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;

    let mut file = fs::File::create(&tmp_path)?;
    file.write_all(json.as_bytes())?;
    file.sync_data()?;
    drop(file);

    fs::rename(&tmp_path, path)?;
    Ok(())
}

/// Load a snapshot from disk.
///
/// Returns `Ok(None)` if the file doesn't exist or if deserialization fails
/// (treating a corrupt snapshot as missing triggers a full rebuild).
pub fn load<S: DeserializeOwned>(path: &Path) -> io::Result<Option<Snapshot<S>>> {
    let contents = match fs::read_to_string(path) {
        Ok(c) => c,
        Err(e) if e.kind() == io::ErrorKind::NotFound => return Ok(None),
        Err(e) => return Err(e),
    };

    match serde_json::from_str(&contents) {
        Ok(snapshot) => Ok(Some(snapshot)),
        Err(_) => Ok(None),
    }
}

/// Delete a snapshot file and its `.tmp` file if present.
///
/// Idempotent — does not error if the files don't exist.
pub fn delete(path: &Path) -> io::Result<()> {
    match fs::remove_file(path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    let tmp_path = path.with_extension("json.tmp");
    match fs::remove_file(&tmp_path) {
        Ok(()) => {}
        Err(e) if e.kind() == io::ErrorKind::NotFound => {}
        Err(e) => return Err(e),
    }

    Ok(())
}
