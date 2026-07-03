use std::io;
use std::path::Path;

use ctx_models::{FileStats, StatsSkipReason};

use crate::lines::count_lines;

pub fn collect_file_stats(
    path: &Path,
    max_file_size: u64,
    test_ctx: Option<&ctx_test::TestContext>,
) -> io::Result<FileStats> {
    let metadata = path.metadata()?;

    if !metadata.is_file() {
        return Ok(FileStats {
            lines: 0,
            bytes: metadata.len(),
            tokens: 0,
            is_text: false,
            skipped_reason: Some(StatsSkipReason::NotAFile),
            tests: 0,
            covered_lines: 0,
            coverable_lines: 0,
        });
    }

    let bytes = metadata.len();
    let (covered_lines, coverable_lines) = test_ctx
        .and_then(|ctx| ctx.get_file_coverage(path))
        .unwrap_or((0, 0));

    match ctx_models::read_file_content(path, max_file_size) {
        ctx_models::FileContentResult::Text(content) => {
            let tokens = crate::estimate_tokens(&content);
            let tests = ctx_test::count_tests(path, &content);
            Ok(FileStats {
                lines: count_lines(&content),
                bytes,
                tokens,
                is_text: true,
                skipped_reason: None,
                tests,
                covered_lines,
                coverable_lines,
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
                tests: 0,
                covered_lines,
                coverable_lines,
            })
        }
        ctx_models::FileContentResult::ReadError(err) => {
            Err(io::Error::new(io::ErrorKind::Other, err))
        }
    }
}
