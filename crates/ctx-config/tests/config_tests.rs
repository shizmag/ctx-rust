use std::fs;
use ctx_config::{find_and_load_config, find_config, load_config};
use ctx_models::Mode;

#[test]
fn test_load_config() {
    let temp_dir = std::env::temp_dir().join("ctx_config_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    let config_path = temp_dir.join(".ctxconfig");
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

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_find_config() {
    let temp_dir = std::env::temp_dir().join("ctx_config_find_test");
    let _ = fs::remove_dir_all(&temp_dir);
    
    let sub_dir = temp_dir.join("src/bin/inner");
    fs::create_dir_all(&sub_dir).unwrap();

    let config_path = temp_dir.join(".ctxconfig");
    fs::write(&config_path, "mode = smart\n").unwrap();

    // Verify find_config finds the config when called from deep sub_dir
    let found = find_config(&sub_dir).unwrap();
    assert_eq!(found.canonicalize().unwrap(), config_path.canonicalize().unwrap());

    // Verify find_and_load_config loads it correctly
    let config = find_and_load_config(&sub_dir).unwrap();
    assert_eq!(config.mode, Some(Mode::Smart));

    let _ = fs::remove_dir_all(&temp_dir);
}
