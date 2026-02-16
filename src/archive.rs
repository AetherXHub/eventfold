use std::fs::{File, OpenOptions};
use std::io::{self, BufRead, BufReader, Write};
use std::path::Path;

/// Compress data and append as a new zstd frame to the archive file.
/// Creates the archive file if it doesn't exist.
pub fn append_compressed_frame(archive_path: &Path, data: &[u8]) -> io::Result<()> {
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(archive_path)?;
    let mut encoder = zstd::Encoder::new(file, 3)?;
    encoder.write_all(data)?;
    let file = encoder.finish()?;
    file.sync_data()?;
    Ok(())
}

/// Open the archive and return a streaming decompressor that reads through
/// all concatenated frames as one continuous byte stream.
/// Returns Ok(None) if archive doesn't exist.
pub fn open_archive_reader(archive_path: &Path) -> io::Result<Option<Box<dyn BufRead>>> {
    if !archive_path.exists() {
        return Ok(None);
    }
    let file = File::open(archive_path)?;
    let decoder = zstd::Decoder::new(file)?;
    Ok(Some(Box::new(BufReader::new(decoder))))
}
