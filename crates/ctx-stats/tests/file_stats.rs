use std::fs;

use ctx_models::StatsSkipReason;
use ctx_stats::collect_file_stats;

#[test]
fn counts_lines_and_bytes_for_text_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_text_file.txt");

    fs::write(&path, "one\ntwo\nthree\n").unwrap();

    let stats = collect_file_stats(&path, 1024).unwrap();

    assert_eq!(stats.lines, 3);
    assert!(stats.bytes > 0);
    assert_eq!(stats.tokens, 4); // "one\ntwo\nthree\n" has 14 chars -> (14+3)/4 = 4
    assert!(stats.is_text);
    assert_eq!(stats.skipped_reason, None);
}

#[test]
fn empty_file_has_zero_lines() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_empty_file.txt");

    fs::write(&path, "").unwrap();

    let stats = collect_file_stats(&path, 1024).unwrap();

    assert_eq!(stats.lines, 0);
    assert_eq!(stats.bytes, 0);
    assert!(stats.is_text);
}

#[test]
fn skips_line_count_for_large_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_large_file.txt");

    fs::write(&path, "hello world").unwrap();

    let stats = collect_file_stats(&path, 4).unwrap();

    assert_eq!(stats.lines, 0);
    assert_eq!(stats.skipped_reason, Some(StatsSkipReason::TooLarge));
}

#[test]
fn skips_line_count_for_non_utf8_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_non_utf8_file.bin");

    fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();

    let stats = collect_file_stats(&path, 1024).unwrap();

    assert_eq!(stats.lines, 0);
    assert!(!stats.is_text);
    assert_eq!(stats.skipped_reason, Some(StatsSkipReason::NonUtf8));
}
