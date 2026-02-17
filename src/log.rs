use crate::archive;
use crate::event::Event;
use crate::view::{ReduceFn, View, ViewOps};
use fs2::FileExt;
use notify::{EventKind, RecursiveMode, Watcher};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc;
use std::time::Duration;

/// Boxed iterator over `(Event, line_hash)` pairs from `read_full()`.
type FullEventIter = Box<dyn Iterator<Item = io::Result<(Event, String)>>>;

/// Controls file locking behavior for an [`EventWriter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum LockMode {
    /// Acquire an exclusive advisory lock on `app.jsonl`.
    /// Prevents other processes from opening a writer on the same file.
    /// This is the default.
    #[default]
    Flock,

    /// No locking. Use when you know only one process accesses the log,
    /// or in test scenarios where multiple writers are intentionally used.
    None,
}

/// Result of waiting for new events.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum WaitResult {
    /// New data appeared in the active log. Contains the new file size.
    NewData(u64),
    /// The timeout elapsed with no new data.
    Timeout,
}

/// Conflict details when a conditional append fails.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AppendConflict {
    /// The offset the caller expected the log to be at.
    pub expected_offset: u64,
    /// The actual current offset (file size).
    pub actual_offset: u64,
    /// The hash the caller expected.
    pub expected_hash: String,
    /// The actual hash of the last line, if the offset matched
    /// but the hash didn't. `None` if the offset check failed first.
    pub actual_hash: Option<String>,
}

/// Error type for conditional append operations.
#[derive(Debug)]
pub enum ConditionalAppendError {
    /// The log state didn't match expectations — no write occurred.
    Conflict(AppendConflict),
    /// An I/O error occurred during the check or write.
    Io(io::Error),
}

impl std::fmt::Display for ConditionalAppendError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ConditionalAppendError::Conflict(c) => {
                write!(
                    f,
                    "conditional append conflict: expected offset {} (hash {:?}), actual offset {} (hash {:?})",
                    c.expected_offset, c.expected_hash, c.actual_offset, c.actual_hash
                )
            }
            ConditionalAppendError::Io(e) => write!(f, "I/O error: {e}"),
        }
    }
}

impl std::error::Error for ConditionalAppendError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            ConditionalAppendError::Io(e) => Some(e),
            ConditionalAppendError::Conflict(_) => None,
        }
    }
}

impl From<io::Error> for ConditionalAppendError {
    fn from(e: io::Error) -> Self {
        ConditionalAppendError::Io(e)
    }
}

/// Result of a successful append operation.
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub struct AppendResult {
    /// Byte offset where the event line starts in `app.jsonl`.
    pub start_offset: u64,

    /// Byte offset after the trailing newline — the position where
    /// the next event would begin.
    pub end_offset: u64,

    /// xxh64 hash of the serialized event line (hex-encoded, without
    /// the trailing newline).
    pub line_hash: String,
}

/// Exclusive writer for a single event log file.
///
/// Owns the append file handle and manages log rotation at the file level.
/// For reading, use [`EventReader`] obtained via [`EventWriter::reader`].
pub struct EventWriter {
    file: File,
    log_path: PathBuf,
    archive_path: PathBuf,
    views_dir: PathBuf,
    max_log_size: u64,
}

impl std::fmt::Debug for EventWriter {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventWriter")
            .field("log_path", &self.log_path)
            .field("archive_path", &self.archive_path)
            .field("views_dir", &self.views_dir)
            .field("max_log_size", &self.max_log_size)
            .finish()
    }
}

impl EventWriter {
    /// Open or create an event log directory for writing.
    ///
    /// Creates `dir/`, `dir/views/`, and `dir/app.jsonl` if they don't exist.
    /// Opens `app.jsonl` in append mode and acquires an exclusive advisory lock.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        Self::open_with_lock(dir, LockMode::Flock)
    }

    /// Open or create an event log directory with an explicit lock mode.
    ///
    /// With [`LockMode::Flock`], acquires an exclusive advisory lock on
    /// `app.jsonl`. If another writer holds the lock, returns an error
    /// immediately (non-blocking).
    ///
    /// With [`LockMode::None`], no lock is acquired.
    pub fn open_with_lock(dir: impl AsRef<Path>, lock: LockMode) -> io::Result<Self> {
        let dir = dir.as_ref().to_path_buf();
        let views_dir = dir.join("views");
        let log_path = dir.join("app.jsonl");
        let archive_path = dir.join("archive.jsonl.zst");

        fs::create_dir_all(&views_dir)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        if lock == LockMode::Flock {
            file.try_lock_exclusive().map_err(|e| {
                io::Error::new(
                    io::ErrorKind::AlreadyExists,
                    format!(
                        "another writer holds the lock on {}: {e}",
                        log_path.display()
                    ),
                )
            })?;
        }

        Ok(EventWriter {
            file,
            log_path,
            archive_path,
            views_dir,
            max_log_size: 0,
        })
    }

    /// Append an event to the log.
    ///
    /// Returns an [`AppendResult`] with the start offset, end offset, and line hash.
    /// Does not trigger auto-rotation. For auto-rotation support, use [`EventLog`].
    pub fn append(&mut self, event: &Event) -> io::Result<AppendResult> {
        let (result, _) = self.append_raw(event)?;
        Ok(result)
    }

    /// Append an event and indicate whether rotation is needed.
    ///
    /// Returns `(AppendResult, needs_rotate)`.
    pub(crate) fn append_raw(&mut self, event: &Event) -> io::Result<(AppendResult, bool)> {
        let start_offset = self.file.seek(SeekFrom::End(0))?;
        let json = serde_json::to_string(event)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        let hash = line_hash(json.as_bytes());
        writeln!(self.file, "{json}")?;
        self.file.sync_data()?;
        let end_offset = start_offset + json.len() as u64 + 1; // +1 for '\n'

        let needs_rotate =
            self.max_log_size > 0 && self.active_log_size()? >= self.max_log_size;
        Ok((
            AppendResult {
                start_offset,
                end_offset,
                line_hash: hash,
            },
            needs_rotate,
        ))
    }

    /// Append an event only if the log's current state matches expectations.
    ///
    /// Checks that the active log's file size equals `expected_offset` and
    /// (if non-zero) that the hash of the last event line matches
    /// `expected_hash`. If either check fails, returns
    /// `Err(ConditionalAppendError::Conflict(...))` without writing.
    ///
    /// For an empty log, pass `expected_offset: 0` and `expected_hash: ""`.
    ///
    /// On success, returns the same `AppendResult` as `append()`.
    pub fn append_if(
        &mut self,
        event: &Event,
        expected_offset: u64,
        expected_hash: &str,
    ) -> Result<AppendResult, ConditionalAppendError> {
        let current_size = self.active_log_size()?;

        // Fast path: offset mismatch means someone else wrote.
        if current_size != expected_offset {
            return Err(ConditionalAppendError::Conflict(AppendConflict {
                expected_offset,
                actual_offset: current_size,
                expected_hash: expected_hash.to_string(),
                actual_hash: None,
            }));
        }

        // If log is non-empty, verify the last line hash.
        if expected_offset > 0 {
            let reader = self.reader();
            let actual_hash = reader
                .read_line_hash_before(expected_offset)?
                .unwrap_or_default();
            if actual_hash != expected_hash {
                return Err(ConditionalAppendError::Conflict(AppendConflict {
                    expected_offset,
                    actual_offset: current_size,
                    expected_hash: expected_hash.to_string(),
                    actual_hash: Some(actual_hash),
                }));
            }
        }

        // Checks passed — proceed with normal append.
        Ok(self.append(event)?)
    }

    /// Manually trigger log rotation.
    ///
    /// Refreshes all views from the reader, compresses the active log to the
    /// archive, truncates the active log, and resets all view offsets.
    pub fn rotate(
        &mut self,
        reader: &EventReader,
        views: &mut HashMap<String, Box<dyn ViewOps>>,
    ) -> io::Result<()> {
        // 1. Refresh all views so snapshots reflect everything in app.jsonl
        for view in views.values_mut() {
            view.refresh_boxed(reader)?;
        }

        // 2. Read active log contents
        let contents = fs::read(&self.log_path)?;

        // 3. No-op if empty
        if contents.is_empty() {
            return Ok(());
        }

        // 4. Compress and append to archive
        archive::append_compressed_frame(&self.archive_path, &contents)?;

        // 5. Truncate active log
        self.file.set_len(0)?;
        self.file.sync_data()?;

        // 6. Reset all view offsets and save snapshots
        for view in views.values_mut() {
            view.reset_offset()?;
        }

        Ok(())
    }

    /// Get a cloneable reader pointing at the same log paths.
    pub fn reader(&self) -> EventReader {
        EventReader {
            log_path: self.log_path.clone(),
            archive_path: self.archive_path.clone(),
        }
    }

    /// Returns the path to the data directory.
    pub fn dir(&self) -> &Path {
        self.log_path.parent().unwrap()
    }

    /// Returns the path to the active log file.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Returns the path to the archive file.
    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }

    /// Returns the path to the `views/` directory.
    pub fn views_dir(&self) -> &Path {
        &self.views_dir
    }

    /// Returns the current size of `app.jsonl` in bytes.
    pub fn active_log_size(&self) -> io::Result<u64> {
        Ok(fs::metadata(&self.log_path)?.len())
    }

    /// Set the maximum active log size for auto-rotation checks.
    pub(crate) fn set_max_log_size(&mut self, bytes: u64) {
        self.max_log_size = bytes;
    }
}

/// Cheap, cloneable reader for an event log.
///
/// Opens fresh file handles per read call. Safe to use concurrently
/// with an [`EventWriter`] on the same log — completed lines are immutable,
/// and partial lines at EOF are detected and skipped.
#[derive(Debug, Clone)]
pub struct EventReader {
    log_path: PathBuf,
    archive_path: PathBuf,
}

impl EventReader {
    /// Create a reader pointing at the given log directory.
    pub fn new(dir: impl AsRef<Path>) -> Self {
        let dir = dir.as_ref();
        EventReader {
            log_path: dir.join("app.jsonl"),
            archive_path: dir.join("archive.jsonl.zst"),
        }
    }

    /// Read events from the active log starting at the given byte offset.
    ///
    /// Returns an iterator yielding `(event, next_byte_offset, line_hash)` for
    /// each complete line. Empty lines are skipped. Partial lines (missing
    /// trailing newline) are skipped silently.
    pub fn read_from(
        &self,
        offset: u64,
    ) -> io::Result<impl Iterator<Item = io::Result<(Event, u64, String)>>> {
        let mut file = File::open(&self.log_path)?;
        file.seek(SeekFrom::Start(offset))?;

        let file_len = file.metadata()?.len();
        let reader = BufReader::new(file);

        Ok(LogIterator {
            lines: reader.lines(),
            pos: offset,
            file_len,
        })
    }

    /// Read the full event history: archive (if any) + active log.
    ///
    /// Returns an iterator yielding `(event, line_hash)` for each event
    /// across all archived frames and the current active log.
    pub fn read_full(&self) -> io::Result<FullEventIter> {
        let archive_iter: Box<dyn Iterator<Item = io::Result<(Event, String)>>> =
            match archive::open_archive_reader(&self.archive_path)? {
                Some(reader) => Box::new(EventLineIter {
                    reader,
                    buf: String::new(),
                }),
                None => Box::new(std::iter::empty()),
            };

        let file = File::open(&self.log_path)?;
        let reader = BufReader::new(file);
        let active_iter: Box<dyn Iterator<Item = io::Result<(Event, String)>>> =
            Box::new(EventLineIter {
                reader,
                buf: String::new(),
            });

        Ok(Box::new(archive_iter.chain(active_iter)))
    }

    /// Read the line immediately before the given byte offset and return its hash.
    ///
    /// The offset should point to the byte after the newline of the last consumed line.
    /// Returns `None` if offset is 0 or beyond the file.
    pub fn read_line_hash_before(&self, offset: u64) -> io::Result<Option<String>> {
        if offset == 0 {
            return Ok(None);
        }

        let mut file = File::open(&self.log_path)?;
        let file_len = file.metadata()?.len();

        if offset > file_len {
            return Ok(None);
        }

        // offset - 1 is the '\n' at end of previous line
        // Scan backwards from offset - 2 to find start of that line
        let newline_pos = offset - 1;
        let mut start = 0u64;

        if newline_pos > 0 {
            let scan_start = newline_pos.saturating_sub(8192);
            file.seek(SeekFrom::Start(scan_start))?;
            let mut buf = vec![0u8; (newline_pos - scan_start) as usize];
            file.read_exact(&mut buf)?;

            if let Some(pos) = buf.iter().rposition(|&b| b == b'\n') {
                start = scan_start + pos as u64 + 1;
            } else {
                start = scan_start;
            }
        }

        file.seek(SeekFrom::Start(start))?;
        let line_len = (newline_pos - start) as usize;
        let mut line_buf = vec![0u8; line_len];
        file.read_exact(&mut line_buf)?;

        Ok(Some(line_hash(&line_buf)))
    }

    /// Returns the current size of `app.jsonl` in bytes.
    ///
    /// This is a lightweight "version" check — if the size hasn't
    /// changed, no new events have been appended.
    pub fn active_log_size(&self) -> io::Result<u64> {
        Ok(fs::metadata(&self.log_path)?.len())
    }

    /// Returns `true` if the active log contains data beyond `offset`.
    ///
    /// This is a non-blocking metadata check (stat call). Use it to
    /// implement poll-based tailing:
    ///
    /// ```no_run
    /// # use eventfold::EventReader;
    /// let reader = EventReader::new("./data");
    /// let mut offset = 0u64;
    /// loop {
    ///     if reader.has_new_events(offset).unwrap() {
    ///         for result in reader.read_from(offset).unwrap() {
    ///             let (event, next_offset, _hash) = result.unwrap();
    ///             // process event
    ///             offset = next_offset;
    ///         }
    ///     }
    ///     std::thread::sleep(std::time::Duration::from_millis(50));
    /// }
    /// ```
    pub fn has_new_events(&self, offset: u64) -> io::Result<bool> {
        Ok(fs::metadata(&self.log_path)?.len() > offset)
    }

    /// Block until new data appears after `offset` in the active log,
    /// or until `timeout` elapses.
    ///
    /// Uses OS-level file system notifications (inotify on Linux,
    /// kqueue on macOS, ReadDirectoryChangesW on Windows) for
    /// near-zero-latency detection.
    ///
    /// # Example
    ///
    /// ```no_run
    /// # use eventfold::{EventReader, WaitResult};
    /// # use std::time::Duration;
    /// let reader = EventReader::new("./data");
    /// let mut offset = 0u64;
    /// loop {
    ///     match reader.wait_for_events(offset, Duration::from_secs(5)).unwrap() {
    ///         WaitResult::NewData(new_size) => {
    ///             for result in reader.read_from(offset).unwrap() {
    ///                 let (event, next_offset, _hash) = result.unwrap();
    ///                 // process event
    ///                 offset = next_offset;
    ///             }
    ///         }
    ///         WaitResult::Timeout => {
    ///             // No new events — do periodic housekeeping, etc.
    ///         }
    ///     }
    /// }
    /// ```
    pub fn wait_for_events(
        &self,
        offset: u64,
        timeout: Duration,
    ) -> io::Result<WaitResult> {
        // Check immediately — data may already be available.
        let current_size = self.active_log_size()?;
        if current_size > offset {
            return Ok(WaitResult::NewData(current_size));
        }

        // Set up a file watcher on the log file's parent directory.
        let (tx, rx) = mpsc::channel();
        let mut watcher =
            notify::recommended_watcher(move |res: Result<notify::Event, _>| {
                if let Ok(event) = res
                    && matches!(event.kind, EventKind::Modify(_) | EventKind::Create(_))
                {
                    let _ = tx.send(());
                }
            })
            .map_err(io::Error::other)?;

        watcher
            .watch(
                self.log_path.parent().unwrap_or(&self.log_path),
                RecursiveMode::NonRecursive,
            )
            .map_err(io::Error::other)?;

        // Re-check after watcher is set up (avoid TOCTOU race).
        let current_size = self.active_log_size()?;
        if current_size > offset {
            return Ok(WaitResult::NewData(current_size));
        }

        // Wait for a notification or timeout.
        match rx.recv_timeout(timeout) {
            Ok(()) => {
                let new_size = self.active_log_size()?;
                if new_size > offset {
                    Ok(WaitResult::NewData(new_size))
                } else {
                    // Spurious wakeup (e.g., metadata change, not a write).
                    // For simplicity, return Timeout. Caller will retry.
                    Ok(WaitResult::Timeout)
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => Ok(WaitResult::Timeout),
            Err(mpsc::RecvTimeoutError::Disconnected) => {
                Err(io::Error::other("file watcher disconnected"))
            }
        }
    }

    /// Returns the path to the active log file.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Returns the path to the archive file.
    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }
}

/// An append-only event log backed by files in a single directory.
///
/// The log manages an active log file (`app.jsonl`), a compressed archive
/// (`archive.jsonl.zst`), a views directory for snapshots, and an optional
/// set of registered views for auto-rotation and bulk refresh.
///
/// Composes an [`EventWriter`] and [`EventReader`] with a view registry.
/// For advanced use cases (multiple readers, direct writer access), use
/// [`EventWriter`] and [`EventReader`] directly.
///
/// Use [`EventLog::builder`] to configure views and auto-rotation, or
/// [`EventLog::open`] for a bare log without registered views.
///
/// # Examples
///
/// ```
/// # use tempfile::tempdir;
/// use eventfold::{Event, EventLog};
/// use serde_json::json;
///
/// # let dir = tempdir().unwrap();
/// let mut log = EventLog::open(dir.path()).unwrap();
/// log.append(&Event::new("click", json!({"x": 10}))).unwrap();
///
/// let events: Vec<_> = log.read_from(0).unwrap()
///     .collect::<Result<Vec<_>, _>>().unwrap();
/// assert_eq!(events.len(), 1);
/// ```
pub struct EventLog {
    writer: EventWriter,
    reader: EventReader,
    views: HashMap<String, Box<dyn ViewOps>>,
}

impl std::fmt::Debug for EventLog {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventLog")
            .field("writer", &self.writer)
            .field("reader", &self.reader)
            .field("view_count", &self.views.len())
            .finish()
    }
}

/// A factory closure that creates a boxed view given a views directory path.
type ViewFactory = Box<dyn FnOnce(&Path) -> Box<dyn ViewOps>>;

/// Builder for configuring and opening an [`EventLog`].
///
/// Register views and set auto-rotation thresholds before calling
/// [`open`](EventLogBuilder::open) to create the log.
///
/// # Examples
///
/// ```
/// # use tempfile::tempdir;
/// # use eventfold::{Event, EventLog};
/// # use serde::{Serialize, Deserialize};
/// # #[derive(Default, Clone, Serialize, Deserialize)]
/// # struct Counter { count: u64 }
/// # fn count(mut s: Counter, _e: &Event) -> Counter { s.count += 1; s }
/// # let dir = tempdir().unwrap();
/// let mut log = EventLog::builder(dir.path())
///     .max_log_size(10_000_000)  // auto-rotate at 10 MB
///     .view::<Counter>("counter", count)
///     .open()
///     .unwrap();
/// ```
pub struct EventLogBuilder {
    dir: PathBuf,
    max_log_size: u64,
    lock_mode: LockMode,
    view_factories: Vec<ViewFactory>,
}

impl std::fmt::Debug for EventLogBuilder {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("EventLogBuilder")
            .field("dir", &self.dir)
            .field("max_log_size", &self.max_log_size)
            .field("lock_mode", &self.lock_mode)
            .field("view_count", &self.view_factories.len())
            .finish()
    }
}

impl EventLogBuilder {
    /// Set the maximum active log size in bytes before auto-rotation triggers.
    /// A value of 0 (the default) disables auto-rotation.
    pub fn max_log_size(mut self, bytes: u64) -> Self {
        self.max_log_size = bytes;
        self
    }

    /// Set the file locking mode. Default is [`LockMode::Flock`].
    pub fn lock_mode(mut self, mode: LockMode) -> Self {
        self.lock_mode = mode;
        self
    }

    /// Register a view with the given name and reducer function.
    pub fn view<S>(mut self, name: &str, reducer: ReduceFn<S>) -> Self
    where
        S: Serialize + DeserializeOwned + Default + Clone + 'static,
    {
        let name = name.to_string();
        self.view_factories.push(Box::new(move |views_dir| {
            Box::new(View::new(&name, reducer, views_dir))
        }));
        self
    }

    /// Open (or create) the event log with the configured settings.
    ///
    /// Creates the directory structure, initializes all registered views,
    /// and performs auto-rotation if the active log exceeds `max_log_size`.
    pub fn open(self) -> io::Result<EventLog> {
        let mut writer = EventWriter::open_with_lock(&self.dir, self.lock_mode)?;
        writer.set_max_log_size(self.max_log_size);
        let reader = writer.reader();

        let mut views = HashMap::new();
        for factory in self.view_factories {
            let view = factory(writer.views_dir());
            views.insert(view.view_name().to_string(), view);
        }

        let mut log = EventLog {
            writer,
            reader,
            views,
        };

        if log.writer.max_log_size > 0
            && log.reader.active_log_size()? >= log.writer.max_log_size
        {
            log.rotate()?;
        }

        Ok(log)
    }
}

/// Compute xxh64 hash of raw line bytes (without trailing newline), hex-encoded.
pub fn line_hash(line: &[u8]) -> String {
    let hash = xxhash_rust::xxh64::xxh64(line, 0);
    format!("{:016x}", hash)
}

impl EventLog {
    /// Open or create an event log in the given directory.
    ///
    /// Creates the directory and `views/` subdirectory if they don't exist.
    /// Opens or creates `app.jsonl` in append mode.
    pub fn open(dir: impl AsRef<Path>) -> io::Result<Self> {
        let writer = EventWriter::open(dir)?;
        let reader = writer.reader();
        Ok(EventLog {
            writer,
            reader,
            views: HashMap::new(),
        })
    }

    /// Create a builder for configuring and opening an event log.
    pub fn builder(dir: impl AsRef<Path>) -> EventLogBuilder {
        EventLogBuilder {
            dir: dir.as_ref().to_path_buf(),
            max_log_size: 0,
            lock_mode: LockMode::default(),
            view_factories: Vec::new(),
        }
    }

    /// Append an event to the active log.
    ///
    /// Serializes the event as a single JSON line, appends it to `app.jsonl`,
    /// and flushes to disk. Returns an [`AppendResult`] with the start offset,
    /// end offset, and line hash.
    /// May trigger auto-rotation if `max_log_size` is configured and exceeded.
    pub fn append(&mut self, event: &Event) -> io::Result<AppendResult> {
        let (result, needs_rotate) = self.writer.append_raw(event)?;
        if needs_rotate {
            self.rotate()?;
        }
        Ok(result)
    }

    /// Conditional append — delegates to the inner writer.
    ///
    /// Appends an event only if the log's current state matches expectations.
    /// May trigger auto-rotation if `max_log_size` is configured and exceeded.
    pub fn append_if(
        &mut self,
        event: &Event,
        expected_offset: u64,
        expected_hash: &str,
    ) -> Result<AppendResult, ConditionalAppendError> {
        let result = self.writer.append_if(event, expected_offset, expected_hash)?;
        if self.writer.max_log_size > 0
            && self.writer.active_log_size()? >= self.writer.max_log_size
        {
            self.rotate()?;
        }
        Ok(result)
    }

    /// Read events from the active log starting at the given byte offset.
    ///
    /// Returns an iterator yielding `(event, next_byte_offset, line_hash)` for
    /// each complete line. Empty lines are skipped. Partial lines (missing
    /// trailing newline) are skipped silently.
    pub fn read_from(
        &self,
        offset: u64,
    ) -> io::Result<impl Iterator<Item = io::Result<(Event, u64, String)>>> {
        self.reader.read_from(offset)
    }

    /// Read the full event history: archive (if any) + active log.
    ///
    /// Returns an iterator yielding `(event, line_hash)` for each event
    /// across all archived frames and the current active log.
    pub fn read_full(&self) -> io::Result<FullEventIter> {
        self.reader.read_full()
    }

    /// Rotate the active log: refresh registered views, compress to archive,
    /// truncate, and reset view offsets.
    ///
    /// If the active log is empty, this is a no-op.
    pub fn rotate(&mut self) -> io::Result<()> {
        self.writer.rotate(&self.reader, &mut self.views)
    }

    /// Refresh all registered views from the event log.
    pub fn refresh_all(&mut self) -> io::Result<()> {
        for view in self.views.values_mut() {
            view.refresh_boxed(&self.reader)?;
        }
        Ok(())
    }

    /// Get a reference to a registered view's current state by name.
    ///
    /// Returns an error if the view name is not found or if the type `S`
    /// does not match the view's actual state type.
    pub fn view<S>(&self, name: &str) -> io::Result<&S>
    where
        S: Serialize + DeserializeOwned + Default + Clone + 'static,
    {
        let view = self.views.get(name).ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::NotFound,
                format!("view '{name}' not found"),
            )
        })?;
        let typed = view
            .as_any()
            .downcast_ref::<View<S>>()
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("view '{name}' type mismatch"),
                )
            })?;
        Ok(typed.state())
    }

    /// Get a cloneable reader for this log.
    pub fn reader(&self) -> EventReader {
        self.reader.clone()
    }

    /// Get a reference to the inner writer.
    pub fn writer(&self) -> &EventWriter {
        &self.writer
    }

    /// Get a mutable reference to the inner writer.
    pub fn writer_mut(&mut self) -> &mut EventWriter {
        &mut self.writer
    }

    /// Returns the path to the data directory.
    pub fn dir(&self) -> &Path {
        self.writer.dir()
    }

    /// Returns the path to the active log file.
    pub fn log_path(&self) -> &Path {
        self.writer.log_path()
    }

    /// Returns the path to the archive file.
    pub fn archive_path(&self) -> &Path {
        self.writer.archive_path()
    }

    /// Returns the path to the views directory.
    pub fn views_dir(&self) -> &Path {
        self.writer.views_dir()
    }

    /// Returns the current size in bytes of the active log file.
    pub fn active_log_size(&self) -> io::Result<u64> {
        self.reader.active_log_size()
    }

    /// Returns `true` if there are events beyond `offset` in the active log.
    pub fn has_new_events(&self, offset: u64) -> io::Result<bool> {
        self.reader.has_new_events(offset)
    }

    /// Block until new data appears after `offset`, or until `timeout` elapses.
    ///
    /// Delegates to [`EventReader::wait_for_events`].
    pub fn wait_for_events(
        &self,
        offset: u64,
        timeout: Duration,
    ) -> io::Result<WaitResult> {
        self.reader.wait_for_events(offset, timeout)
    }

    /// Read the line immediately before the given byte offset and return its hash.
    ///
    /// The offset should point to the byte after the newline of the last consumed line.
    /// Returns `None` if offset is 0.
    pub fn read_line_hash_before(&self, offset: u64) -> io::Result<Option<String>> {
        self.reader.read_line_hash_before(offset)
    }
}

struct LogIterator<I> {
    lines: I,
    pos: u64,
    file_len: u64,
}

impl<I: Iterator<Item = io::Result<String>>> Iterator for LogIterator<I> {
    type Item = io::Result<(Event, u64, String)>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let line = match self.lines.next()? {
                Ok(line) => line,
                Err(e) => return Some(Err(e)),
            };

            let line_bytes = line.len() as u64;

            // If the line content reaches exactly EOF without a trailing newline,
            // this is a partial line (crash mid-write) — skip it.
            if self.pos + line_bytes >= self.file_len {
                return None;
            }

            // Advance position past line + newline
            let next_pos = self.pos + line_bytes + 1; // +1 for the newline

            // Skip empty lines
            if line.is_empty() {
                self.pos = next_pos;
                continue;
            }

            let hash = line_hash(line.as_bytes());

            let event: Event = match serde_json::from_str(&line) {
                Ok(e) => e,
                Err(e) => {
                    return Some(Err(io::Error::new(io::ErrorKind::InvalidData, e)));
                }
            };

            self.pos = next_pos;
            return Some(Ok((event, next_pos, hash)));
        }
    }
}

/// Iterator that reads events line-by-line from any BufRead source.
/// Used by `read_full()` for both archive and active log streams.
struct EventLineIter<R> {
    reader: R,
    buf: String,
}

impl<R: BufRead> Iterator for EventLineIter<R> {
    type Item = io::Result<(Event, String)>;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            self.buf.clear();
            match self.reader.read_line(&mut self.buf) {
                Ok(0) => return None,
                Ok(_) => {
                    // Skip partial lines at EOF (no trailing newline — crash mid-write)
                    if !self.buf.ends_with('\n') {
                        return None;
                    }
                    let line = self.buf.trim_end_matches('\n').trim_end_matches('\r');
                    if line.is_empty() {
                        continue;
                    }
                    let hash = line_hash(line.as_bytes());
                    match serde_json::from_str::<Event>(line) {
                        Ok(event) => return Some(Ok((event, hash))),
                        Err(e) => {
                            return Some(Err(io::Error::new(io::ErrorKind::InvalidData, e)))
                        }
                    }
                }
                Err(e) => return Some(Err(e)),
            }
        }
    }
}
