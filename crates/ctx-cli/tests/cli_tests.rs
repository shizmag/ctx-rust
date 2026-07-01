use std::fs;
use std::process::Command;

#[test]
fn test_cli_help() {
    let output = Command::new("cargo")
        .args(&["run", "--bin", "ctx", "--", "--help"])
        .output()
        .expect("failed to execute cargo run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Context gatherer for LLMs"));
}

#[test]
fn test_cli_scan() {
    let temp_dir = std::env::temp_dir().join("ctx_cli_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("a.rs"), "fn main() {}\n").unwrap();

    let output = Command::new("cargo")
        .args(&[
            "run",
            "--bin",
            "ctx",
            "--",
            temp_dir.to_str().unwrap(),
            "--format",
            "plain",
        ])
        .output()
        .expect("failed to execute cargo run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("File: a.rs"));
    assert!(stdout.contains("fn main() {}"));

    let _ = fs::remove_dir_all(&temp_dir);
}
