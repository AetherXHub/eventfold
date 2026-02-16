use crate::archive;
use crate::event::Event;
use crate::view::{ReduceFn, View, ViewOps};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::collections::HashMap;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

/// Boxed iterator over `(Event, line_hash)` pairs from `read_full()`.
type FullEventIter = Box<dyn Iterator<Item = io::Result<(Event, String)>>>;

pub struct EventLog {
    dir: PathBuf,
    log_path: PathBuf,
    archive_path: PathBuf,
    file: File,
    views_dir: PathBuf,
    views: HashMap<String, Box<dyn ViewOps>>,
    max_log_size: u64,
}

/// A factory closure that creates a boxed view given a views directory path.
type ViewFactory = Box<dyn FnOnce(&Path) -> Box<dyn ViewOps>>;

/// Builder for configuring and opening an `EventLog`.
pub struct EventLogBuilder {
    dir: PathBuf,
    max_log_size: u64,
    view_factories: Vec<ViewFactory>,
}

impl EventLogBuilder {
    /// Set the maximum active log size in bytes before auto-rotation triggers.
    /// A value of 0 (the default) disables auto-rotation.
    pub fn max_log_size(mut self, bytes: u64) -> Self {
        self.max_log_size = bytes;
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
        let dir = self.dir;
        let views_dir = dir.join("views");
        let log_path = dir.join("app.jsonl");
        let archive_path = dir.join("archive.jsonl.zst");

        fs::create_dir_all(&views_dir)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        let mut views = HashMap::new();
        for factory in self.view_factories {
            let view = factory(&views_dir);
            views.insert(view.view_name().to_string(), view);
        }

        let mut log = EventLog {
            dir,
            log_path,
            archive_path,
            file,
            views_dir,
            views,
            max_log_size: self.max_log_size,
        };

        if log.max_log_size > 0 && log.active_log_size()? >= log.max_log_size {
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
        let dir = dir.as_ref().to_path_buf();
        let views_dir = dir.join("views");
        let log_path = dir.join("app.jsonl");
        let archive_path = dir.join("archive.jsonl.zst");

        fs::create_dir_all(&views_dir)?;

        let file = OpenOptions::new()
            .create(true)
            .append(true)
            .open(&log_path)?;

        Ok(EventLog {
            dir,
            log_path,
            archive_path,
            file,
            views_dir,
            views: HashMap::new(),
            max_log_size: 0,
        })
    }

    /// Create a builder for configuring and opening an event log.
    pub fn builder(dir: impl AsRef<Path>) -> EventLogBuilder {
        EventLogBuilder {
            dir: dir.as_ref().to_path_buf(),
            max_log_size: 0,
            view_factories: Vec::new(),
        }
    }

    /// Append an event to the active log.
    ///
    /// Serializes the event as a single JSON line, appends it to `app.jsonl`,
    /// and flushes to disk. Returns the byte offset where the event starts.
    /// May trigger auto-rotation if `max_log_size` is configured and exceeded.
    pub fn append(&mut self, event: &Event) -> io::Result<u64> {
        let offset = self.file.seek(SeekFrom::End(0))?;
        let json = serde_json::to_string(event)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        writeln!(self.file, "{json}")?;
        self.file.sync_data()?;

        if self.max_log_size > 0 && self.active_log_size()? >= self.max_log_size {
            self.rotate()?;
        }

        Ok(offset)
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

    /// Rotate the active log: refresh registered views, compress to archive,
    /// truncate, and reset view offsets.
    ///
    /// If the active log is empty, this is a no-op.
    pub fn rotate(&mut self) -> io::Result<()> {
        let mut views = std::mem::take(&mut self.views);
        let result = self.rotate_inner(&mut views);
        self.views = views;
        result
    }

    fn rotate_inner(
        &mut self,
        views: &mut HashMap<String, Box<dyn ViewOps>>,
    ) -> io::Result<()> {
        // 1. Refresh all views so snapshots reflect everything in app.jsonl
        for view in views.values_mut() {
            view.refresh_boxed(self)?;
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

    /// Refresh all registered views from the event log.
    pub fn refresh_all(&mut self) -> io::Result<()> {
        let mut views = std::mem::take(&mut self.views);
        let result = (|| {
            for view in views.values_mut() {
                view.refresh_boxed(self)?;
            }
            Ok(())
        })();
        self.views = views;
        result
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

    /// Returns the path to the data directory.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Returns the path to the active log file.
    pub fn log_path(&self) -> &Path {
        &self.log_path
    }

    /// Returns the path to the archive file.
    pub fn archive_path(&self) -> &Path {
        &self.archive_path
    }

    /// Returns the path to the views directory.
    pub fn views_dir(&self) -> &Path {
        &self.views_dir
    }

    /// Returns the current size in bytes of the active log file.
    pub fn active_log_size(&self) -> io::Result<u64> {
        Ok(fs::metadata(&self.log_path)?.len())
    }

    /// Read the line immediately before the given byte offset and return its hash.
    ///
    /// The offset should point to the byte after the newline of the last consumed line.
    /// Returns `None` if offset is 0.
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
