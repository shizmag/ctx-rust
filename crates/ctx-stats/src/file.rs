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
            tokens: 0,
            is_text: false,
            skipped_reason: Some(StatsSkipReason::NotAFile),
        });
    }

    let bytes = metadata.len();

    match ctx_models::read_file_content(path, max_file_size) {
        ctx_models::FileContentResult::Text(content) => {
            let tokens = crate::estimate_tokens(&content);
            Ok(FileStats {
                lines: count_lines(&content),
                bytes,
                tokens,
                is_text: true,
                skipped_reason: None,
            })
        }
        ctx_models::FileContentResult::Skipped(reason) => {
            let skipped_reason = match reason {
                ctx_models::FileSkipReason::TooLarge => StatsSkipReason::TooLarge,
                ctx_models::FileSkipReason::NonUtf8 => StatsSkipReason::NonUtf8,
            };
            Ok(FileStats {
                lines: 0,
                bytes,
                tokens: 0,
                is_text: false,
                skipped_reason: Some(skipped_reason),
            })
        }
        ctx_models::FileContentResult::ReadError(err) => {
            Err(io::Error::new(io::ErrorKind::Other, err))
        }
    }
}
