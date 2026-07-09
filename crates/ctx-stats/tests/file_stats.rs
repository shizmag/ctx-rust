use std::fs;

use ctx_models::StatsSkipReason;
use ctx_stats::collect_file_stats;

#[test]
fn counts_lines_and_bytes_for_text_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_text_file.txt");

    fs::write(&path, "one\ntwo\nthree\n").unwrap();

    let stats = collect_file_stats(&path, 1024, None).unwrap();

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

    let stats = collect_file_stats(&path, 1024, None).unwrap();

    assert_eq!(stats.lines, 0);
    assert_eq!(stats.bytes, 0);
    assert!(stats.is_text);
}

#[test]
fn skips_line_count_for_large_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_large_file.txt");

    fs::write(&path, "hello world").unwrap();

    let stats = collect_file_stats(&path, 4, None).unwrap();

    assert_eq!(stats.lines, 0);
    assert_eq!(stats.skipped_reason, Some(StatsSkipReason::TooLarge));
}

#[test]
fn skips_line_count_for_non_utf8_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_non_utf8_file.bin");

    fs::write(&path, [0xff, 0xfe, 0xfd]).unwrap();

    let stats = collect_file_stats(&path, 1024, None).unwrap();

    assert_eq!(stats.lines, 0);
    assert!(!stats.is_text);
    assert_eq!(stats.skipped_reason, Some(StatsSkipReason::NonUtf8));
}

#[test]
fn directory_path_returns_not_a_file() {
    let dir = tempfile::tempdir().unwrap();

    let stats = collect_file_stats(dir.path(), 1024, None).unwrap();

    assert_eq!(stats.lines, 0);
    assert_eq!(stats.tokens, 0);
    assert!(!stats.is_text);
    assert_eq!(stats.skipped_reason, Some(StatsSkipReason::NotAFile));
}

#[test]
fn counts_rust_tests_in_source_file() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_test_file.rs");

    fs::write(
        &path,
        r#"
#[test]
fn first_test() {}

#[test]
fn second_test() {}
"#,
    )
    .unwrap();

    let stats = collect_file_stats(&path, 1024, None).unwrap();

    assert_eq!(stats.tests, 2);
    assert_eq!(stats.skipped_reason, None);
}

#[test]
fn file_exactly_at_max_file_size_is_not_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_boundary_file.txt");
    let content = "12345";

    fs::write(&path, content).unwrap();
    let max_file_size = content.len() as u64;

    let stats = collect_file_stats(&path, max_file_size, None).unwrap();

    assert_eq!(stats.bytes, max_file_size);
    assert_eq!(stats.lines, 1);
    assert!(stats.is_text);
    assert_eq!(stats.skipped_reason, None);
}

#[test]
fn file_one_byte_over_max_file_size_is_skipped() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("ctx_stats_over_boundary_file.txt");
    let content = "123456";

    fs::write(&path, content).unwrap();
    let max_file_size = (content.len() - 1) as u64;

    let stats = collect_file_stats(&path, max_file_size, None).unwrap();

    assert_eq!(stats.lines, 0);
    assert_eq!(stats.skipped_reason, Some(StatsSkipReason::TooLarge));
}
