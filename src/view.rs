use crate::event::Event;
use crate::log::EventLog;
use crate::snapshot::{self, Snapshot};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::any::Any;
use std::io;
use std::path::{Path, PathBuf};

/// A pure function that folds an event into state.
///
/// Reducers receive owned state and return owned state. They should be pure
/// (no I/O, no side effects) and always handle unknown event types with a
/// `_ => {}` arm for forward compatibility.
///
/// # Examples
///
/// ```
/// use eventfold::{Event, ReduceFn};
///
/// fn counter(state: u64, _event: &Event) -> u64 {
///     state + 1
/// }
///
/// let reducer: ReduceFn<u64> = counter;
/// ```
pub type ReduceFn<S> = fn(S, &Event) -> S;

/// Trait for type-erased view operations during log rotation.
pub trait ViewOps {
    /// Refresh the view from the log, discarding the state reference.
    fn refresh_boxed(&mut self, log: &EventLog) -> io::Result<()>;
    /// Reset the offset to 0 and save the snapshot.
    fn reset_offset(&mut self) -> io::Result<()>;
    /// Returns the view name.
    fn view_name(&self) -> &str;
    /// Downcast to `&dyn Any` for type recovery.
    fn as_any(&self) -> &dyn Any;
    /// Downcast to `&mut dyn Any` for type recovery.
    fn as_any_mut(&mut self) -> &mut dyn Any;
}

/// A derived view over an event log.
///
/// Owns a reducer function, manages its snapshot on disk, and supports
/// incremental refresh from the active log.
pub struct View<S> {
    name: String,
    reducer: ReduceFn<S>,
    snapshot_path: PathBuf,
    state: S,
    offset: u64,
    hash: String,
    loaded: bool,
    needs_full_replay: bool,
}

impl<S> View<S>
where
    S: Serialize + DeserializeOwned + Default + Clone,
{
    /// Create a new view.
    ///
    /// `name` identifies this view (used for the snapshot filename).
    /// `reducer` is the fold function applied to each event.
    /// `views_dir` is the directory where snapshot files are stored.
    pub fn new(name: &str, reducer: ReduceFn<S>, views_dir: &Path) -> Self {
        let snapshot_path = views_dir.join(format!("{name}.snapshot.json"));
        View {
            name: name.to_string(),
            reducer,
            snapshot_path,
            state: S::default(),
            offset: 0,
            hash: String::new(),
            loaded: false,
            needs_full_replay: false,
        }
    }

    /// Refresh the view from the event log.
    ///
    /// On first call, attempts to load a snapshot from disk. If no snapshot
    /// exists, uses `read_full()` to replay the archive + active log.
    /// If a snapshot exists, reads only new events from the active log.
    pub fn refresh(&mut self, log: &EventLog) -> io::Result<&S> {
        if !self.loaded {
            if let Some(snap) = snapshot::load::<S>(&self.snapshot_path)? {
                self.state = snap.state;
                self.offset = snap.offset;
                self.hash = snap.hash;
            } else {
                self.needs_full_replay = true;
            }
            self.loaded = true;

            // Verify snapshot integrity
            if self.offset > 0 {
                match self.verify_snapshot(log)? {
                    SnapshotValidity::Valid => {}
                    SnapshotValidity::OffsetBeyondEof => {
                        eprintln!(
                            "eventfold: view '{}': snapshot offset {} is beyond log EOF, rebuilding",
                            self.name, self.offset
                        );
                        self.state = S::default();
                        self.offset = 0;
                        self.hash = String::new();
                        self.needs_full_replay = true;
                    }
                    SnapshotValidity::HashMismatch => {
                        eprintln!(
                            "eventfold: view '{}': snapshot hash mismatch, rebuilding",
                            self.name
                        );
                        self.state = S::default();
                        self.offset = 0;
                        self.hash = String::new();
                        self.needs_full_replay = true;
                    }
                }
            }
        }

        let mut state = std::mem::take(&mut self.state);
        let mut new_offset = self.offset;
        let mut new_hash = self.hash.clone();
        let mut processed = false;

        if self.needs_full_replay {
            self.needs_full_replay = false;
            for result in log.read_full()? {
                let (event, line_hash) = result?;
                state = (self.reducer)(state, &event);
                new_hash = line_hash;
                processed = true;
            }
            if processed {
                new_offset = log.active_log_size()?;
            }
        } else {
            for result in log.read_from(self.offset)? {
                let (event, next_offset, line_hash) = result?;
                state = (self.reducer)(state, &event);
                new_offset = next_offset;
                new_hash = line_hash;
                processed = true;
            }
        }

        self.state = state;

        if processed {
            self.offset = new_offset;
            self.hash = new_hash;
            snapshot::save(
                &self.snapshot_path,
                &Snapshot {
                    state: self.state.clone(),
                    offset: self.offset,
                    hash: self.hash.clone(),
                },
            )?;
        }

        Ok(&self.state)
    }

    /// Return a reference to the current in-memory state.
    ///
    /// No I/O — returns whatever state is currently held. If `refresh`
    /// has not been called, returns `S::default()`.
    pub fn state(&self) -> &S {
        &self.state
    }

    /// Rebuild the view by replaying the full history (archive + active log).
    ///
    /// Deletes the existing snapshot, resets state to default, and
    /// calls `refresh` to replay everything.
    pub fn rebuild(&mut self, log: &EventLog) -> io::Result<&S> {
        snapshot::delete(&self.snapshot_path)?;
        self.state = S::default();
        self.offset = 0;
        self.hash = String::new();
        self.loaded = true;
        self.needs_full_replay = true;
        self.refresh(log)
    }

    /// Returns the view name.
    pub fn name(&self) -> &str {
        &self.name
    }

    fn verify_snapshot(&self, log: &EventLog) -> io::Result<SnapshotValidity> {
        let file_size = log.active_log_size()?;

        if self.offset > file_size {
            return Ok(SnapshotValidity::OffsetBeyondEof);
        }

        if self.offset == 0 {
            return Ok(SnapshotValidity::Valid);
        }

        match log.read_line_hash_before(self.offset)? {
            Some(hash) if hash == self.hash => Ok(SnapshotValidity::Valid),
            Some(_) => Ok(SnapshotValidity::HashMismatch),
            None => Ok(SnapshotValidity::Valid),
        }
    }
}

impl<S> ViewOps for View<S>
where
    S: Serialize + DeserializeOwned + Default + Clone + 'static,
{
    fn refresh_boxed(&mut self, log: &EventLog) -> io::Result<()> {
        self.refresh(log)?;
        Ok(())
    }

    fn reset_offset(&mut self) -> io::Result<()> {
        self.offset = 0;
        self.hash = String::new();
        snapshot::save(
            &self.snapshot_path,
            &Snapshot {
                state: self.state.clone(),
                offset: self.offset,
                hash: self.hash.clone(),
            },
        )
    }

    fn view_name(&self) -> &str {
        &self.name
    }

    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }
}

enum SnapshotValidity {
    Valid,
    OffsetBeyondEof,
    HashMismatch,
}
