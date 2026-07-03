use ctx_models::{FileStats, ProjectSummary};

pub fn increment_hidden(summary: &mut ProjectSummary, is_dir: bool) {
    if is_dir {
        summary.hidden_dirs += 1;
    } else {
        summary.hidden_files += 1;
    }
}

pub fn add_dir(summary: &mut ProjectSummary) {
    summary.dirs += 1;
}

pub fn add_file(summary: &mut ProjectSummary, file_stats: &FileStats) {
    summary.files += 1;
    summary.lines += file_stats.lines;
    summary.bytes += file_stats.bytes;
    summary.tokens += file_stats.tokens;
}
