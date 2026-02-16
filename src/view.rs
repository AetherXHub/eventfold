use crate::event::Event;
use crate::log::EventLog;
use crate::snapshot::{self, Snapshot};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io;
use std::path::{Path, PathBuf};

/// A plain function pointer that folds an event into state.
pub type ReduceFn<S> = fn(S, &Event) -> S;

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
        }
    }

    /// Refresh the view from the event log.
    ///
    /// On first call, attempts to load a snapshot from disk. Then reads
    /// any new events from the log starting at the current offset, folds
    /// them through the reducer, and saves a new snapshot if events were
    /// processed.
    pub fn refresh(&mut self, log: &EventLog) -> io::Result<&S> {
        if !self.loaded {
            if let Some(snap) = snapshot::load::<S>(&self.snapshot_path)? {
                self.state = snap.state;
                self.offset = snap.offset;
                self.hash = snap.hash;
            }
            self.loaded = true;
        }

        let mut state = std::mem::take(&mut self.state);
        let mut new_offset = self.offset;
        let mut new_hash = self.hash.clone();
        let mut processed = false;

        for result in log.read_from(self.offset)? {
            let (event, next_offset, line_hash) = result?;
            state = (self.reducer)(state, &event);
            new_offset = next_offset;
            new_hash = line_hash;
            processed = true;
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

    /// Rebuild the view by replaying the full active log.
    ///
    /// Deletes the existing snapshot, resets state to default, and
    /// calls `refresh` to replay from byte 0.
    pub fn rebuild(&mut self, log: &EventLog) -> io::Result<&S> {
        snapshot::delete(&self.snapshot_path)?;
        self.state = S::default();
        self.offset = 0;
        self.hash = String::new();
        self.loaded = true;
        self.refresh(log)
    }

    /// Returns the view name.
    pub fn name(&self) -> &str {
        &self.name
    }
}
