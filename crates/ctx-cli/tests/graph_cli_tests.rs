use std::fs;
use std::path::Path;

fn create_temp_project(root: &Path) {
    let cargo_content = r#"
        [package]
        name = "temp_project"
        version = "0.1.0"
        edition = "2024"
    "#;
    fs::write(root.join("Cargo.toml"), cargo_content).unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let lib_code = r#"
        pub fn run_pipeline() {
            let value = load();
            process(value);
        }

        fn load() -> i32 {
            1
        }

        fn process(value: i32) {
            save(value);
        }

        fn save(_: i32) {}

        fn unrelated() {}
    "#;
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();
}

#[test]
fn test_cli_graph_build() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["graph", "build", root.to_str().unwrap(), "--no-rust-analyzer"])
        .output()
        .expect("failed to run ctx graph build");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Index successfully built") || stdout.contains("codegraph"));
    
    let db_path = root.join(".ctx-codegraph/codegraph.sqlite");
    assert!(db_path.exists());
}

#[test]
fn test_cli_graph_symbols() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    // Verify symbols output (should auto-build index since it doesn't exist)
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["graph", "symbols", root.to_str().unwrap(), "--no-rust-analyzer"])
        .output()
        .expect("failed to run ctx graph symbols");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("run_pipeline"));
    assert!(stdout.contains("load"));
    assert!(stdout.contains("process"));
}

#[test]
fn test_cli_graph_calls() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["graph", "calls", "run_pipeline", root.to_str().unwrap(), "--no-rust-analyzer"])
        .output()
        .expect("failed to run ctx graph calls");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("load"));
    assert!(stdout.contains("process"));
}

#[test]
fn test_cli_graph_callers() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["graph", "callers", "load", root.to_str().unwrap(), "--no-rust-analyzer"])
        .output()
        .expect("failed to run ctx graph callers");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("run_pipeline"));
}

#[test]
fn test_cli_graph_slice() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["graph", "slice", "run_pipeline", root.to_str().unwrap(), "--no-rust-analyzer"])
        .output()
        .expect("failed to run ctx graph slice");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("run_pipeline"));
    assert!(stdout.contains("load"));
    assert!(stdout.contains("process"));
    assert!(stdout.contains("save"));
    assert!(!stdout.contains("unrelated"));
}

#[test]
fn test_cli_ambiguous_symbol_message() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();

    let cargo_content = r#"
        [package]
        name = "temp_project"
        version = "0.1.0"
        edition = "2024"
    "#;
    fs::write(root.join("Cargo.toml"), cargo_content).unwrap();

    let src_dir = root.join("src");
    fs::create_dir_all(&src_dir).unwrap();

    let lib_code = r#"
        pub mod m1 {
            pub fn load() {}
        }
        pub mod m2 {
            pub fn load() {}
        }
        pub fn call_them() {
            m1::load();
            m2::load();
        }
    "#;
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();

    // Call "load" should be ambiguous
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["graph", "calls", "load", root.to_str().unwrap(), "--no-rust-analyzer"])
        .output()
        .expect("failed to run ctx graph calls load");

    // It should exit with code 1 (failure)
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Ambiguous symbol: load"));
    assert!(stderr.contains("Candidates:"));
    assert!(stderr.contains("m1::load"));
    assert!(stderr.contains("m2::load"));
}

#[test]
fn existing_plain_scan_still_works_after_graph_subcommands() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();
    fs::write(temp_path.join("lib.rs"), "fn main() {}\n").unwrap();

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd.arg(temp_path).output().expect("failed to execute ctx");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("lib.rs"));
    assert!(stdout.contains("Project Summary:"));
}
