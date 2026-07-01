use std::fs;
use std::io;
use std::path::Path;

use ctx_models::{FileStats, StatsSkipReason};

use crate::lines::count_lines;

pub fn collect_file_stats(path: &Path, max_file_size: u64) -> io::Result<FileStats> {
    let metadata = path.metadata()?;

    if !metadata.is_file() {
        return Ok(FileStats {
            lines: 0,
            bytes: metadata.len(),
            is_text: false,
            skipped_reason: Some(StatsSkipReason::NotAFile),
        });
    }

    let bytes = metadata.len();
    if bytes > max_file_size {
        return Ok(FileStats {
            lines: 0,
            bytes,
            is_text: false,
            skipped_reason: Some(StatsSkipReason::TooLarge),
        });
    }

    let content = match fs::read_to_string(path) {
        Ok(content) => content,
        Err(err) if err.kind() == io::ErrorKind::InvalidData => {
            return Ok(FileStats {
                bytes,
                lines: 0,
                is_text: false,
                skipped_reason: Some(StatsSkipReason::NonUtf8),
            });
        }
        Err(err) => {
            return Err(err);
        }
    };

    Ok(FileStats {
        lines: count_lines(&content),
        bytes,
        is_text: true,
        skipped_reason: None,
    })
}
