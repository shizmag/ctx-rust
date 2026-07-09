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
    summary.tests += file_stats.tests;
    summary.covered_lines += file_stats.covered_lines;
    summary.coverable_lines += file_stats.coverable_lines;
}

#[cfg(test)]
mod tests {
    use super::*;
    use ctx_models::FileStats;

    #[test]
    fn increment_hidden_counts_directories() {
        let mut summary = ProjectSummary::default();
        increment_hidden(&mut summary, true);
        assert_eq!(summary.hidden_dirs, 1);
        assert_eq!(summary.hidden_files, 0);
    }

    #[test]
    fn increment_hidden_counts_files() {
        let mut summary = ProjectSummary::default();
        increment_hidden(&mut summary, false);
        assert_eq!(summary.hidden_dirs, 0);
        assert_eq!(summary.hidden_files, 1);
    }

    #[test]
    fn add_dir_increments_dir_count() {
        let mut summary = ProjectSummary::default();
        add_dir(&mut summary);
        assert_eq!(summary.dirs, 1);
        assert_eq!(summary.files, 0);
    }

    #[test]
    fn add_file_aggregates_all_stats_fields() {
        let mut summary = ProjectSummary::default();
        let file_stats = FileStats {
            lines: 10,
            bytes: 256,
            tokens: 42,
            is_text: true,
            skipped_reason: None,
            tests: 3,
            covered_lines: 7,
            coverable_lines: 9,
        };

        add_file(&mut summary, &file_stats);

        assert_eq!(summary.files, 1);
        assert_eq!(summary.lines, 10);
        assert_eq!(summary.bytes, 256);
        assert_eq!(summary.tokens, 42);
        assert_eq!(summary.tests, 3);
        assert_eq!(summary.covered_lines, 7);
        assert_eq!(summary.coverable_lines, 9);
    }
}
