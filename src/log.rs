use crate::event::Event;
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufRead, BufReader, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};

pub struct EventLog {
    dir: PathBuf,
    log_path: PathBuf,
    archive_path: PathBuf,
    file: File,
    views_dir: PathBuf,
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
        })
    }

    /// Append an event to the active log.
    ///
    /// Serializes the event as a single JSON line, appends it to `app.jsonl`,
    /// and flushes to disk. Returns the byte offset where the event starts.
    pub fn append(&mut self, event: &Event) -> io::Result<u64> {
        let offset = self.file.seek(SeekFrom::End(0))?;
        let json = serde_json::to_string(event)
            .map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))?;
        writeln!(self.file, "{json}")?;
        self.file.sync_data()?;
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
