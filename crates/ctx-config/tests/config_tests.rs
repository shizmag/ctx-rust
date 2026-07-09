use ctx_config::{find_and_load_config, find_config, load_config, Config};
use ctx_models::Mode;
use std::fs;

#[test]
fn test_load_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        r#"
# Test Configuration
mode = code
max_depth = 8
max_file_size = 1048576
exclude = target, node_modules, temp_file.txt
"#,
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.mode, Some(Mode::Code));
    assert_eq!(config.max_depth, Some(8));
    assert_eq!(config.max_file_size, Some(1048576));
    assert_eq!(
        config.exclude,
        vec![
            "target".to_string(),
            "node_modules".to_string(),
            "temp_file.txt".to_string()
        ]
    );
}

#[test]
fn test_find_config() {
    let temp_dir = tempfile::tempdir().unwrap();

    let sub_dir = temp_dir.path().join("src/bin/inner");
    fs::create_dir_all(&sub_dir).unwrap();

    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(&config_path, "mode = smart\n").unwrap();

    // Verify find_config finds the config when called from deep sub_dir
    let found = find_config(&sub_dir).unwrap();
    assert_eq!(
        found.canonicalize().unwrap(),
        config_path.canonicalize().unwrap()
    );

    // Verify find_and_load_config loads it correctly
    let config = find_and_load_config(&sub_dir).unwrap();
    assert_eq!(config.mode, Some(Mode::Smart));
}

#[test]
fn load_config_missing_file_returns_default() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join("nonexistent.ctxconfig");

    let config = load_config(&config_path).unwrap();

    assert_eq!(config, Config::default());
}

#[test]
fn load_config_ignores_invalid_mode() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(&config_path, "mode = invalid_mode\n").unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.mode, None);
}

#[test]
fn load_config_ignores_invalid_max_depth_and_max_file_size() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        "max_depth = not_a_number\nmax_file_size = also_invalid\n",
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.max_depth, None);
    assert_eq!(config.max_file_size, None);
}

#[test]
fn load_config_empty_exclude_list() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(&config_path, "exclude = \n").unwrap();

    let config = load_config(&config_path).unwrap();

    assert!(config.exclude.is_empty());
}

#[test]
fn load_config_ignores_comments_and_blank_lines() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        r#"
# This is a comment

mode = docs

# Another comment
max_depth = 3
"#,
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(config.mode, Some(Mode::Docs));
    assert_eq!(config.max_depth, Some(3));
    assert_eq!(config.max_file_size, None);
    assert!(config.exclude.is_empty());
}

#[test]
fn find_config_returns_none_when_no_config_exists() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sub_dir = temp_dir.path().join("src/deep");
    fs::create_dir_all(&sub_dir).unwrap();

    let found = find_config(&sub_dir);

    assert!(found.is_none());
}

#[test]
fn find_and_load_config_returns_default_when_no_config() {
    let temp_dir = tempfile::tempdir().unwrap();
    let sub_dir = temp_dir.path().join("nested/dir");
    fs::create_dir_all(&sub_dir).unwrap();

    let config = find_and_load_config(&sub_dir).unwrap();

    assert_eq!(config, Config::default());
}

#[test]
fn load_config_exclude_trims_extra_whitespace() {
    let temp_dir = tempfile::tempdir().unwrap();
    let config_path = temp_dir.path().join(".ctxconfig");
    fs::write(
        &config_path,
        "exclude =  target ,  node_modules  , , temp_file.txt \n",
    )
    .unwrap();

    let config = load_config(&config_path).unwrap();

    assert_eq!(
        config.exclude,
        vec![
            "target".to_string(),
            "node_modules".to_string(),
            "temp_file.txt".to_string()
        ]
    );
}
