use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};
use std::fs;
use std::io::{self, Write};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Snapshot<S> {
    pub state: S,
    pub offset: u64,
    pub hash: String,
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
