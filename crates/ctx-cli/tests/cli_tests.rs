use std::fs;
use std::process::Command;

#[test]
fn test_cli_help() {
    let output = Command::new("cargo")
        .args(["run", "--bin", "ctx", "--", "--help"])
        .output()
        .expect("failed to execute cargo run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("directory tree visualizer"));
}

#[test]
fn test_cli_scan() {
    let temp_dir = std::env::temp_dir().join("ctx_cli_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();
    fs::write(temp_dir.join("a.rs"), "fn main() {}\n").unwrap();

    // 1. Test ordinary call: should print colored tree and summary
    let output = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "ctx",
            "--",
            temp_dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to execute cargo run");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Verify tree/summary containing folder and file details (with Tokyo Night ANSI blue \x1b[1;38;2;122;162;247m)
    assert!(stdout.contains("ctx_cli_test"));
    assert!(stdout.contains("a.rs"));
    assert!(stdout.contains("Project Summary:"));

    // 2. Test call with -C (code): should print the full code content
    let output_code = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "ctx",
            "--",
            temp_dir.to_str().unwrap(),
            "--format",
            "plain",
            "-C",
        ])
        .output()
        .expect("failed to execute cargo run");

    assert!(output_code.status.success());
    let stdout_code = String::from_utf8(output_code.stdout).unwrap();
    assert!(stdout_code.contains("File: a.rs"));
    assert!(stdout_code.contains("fn main() {}"));

    let _ = fs::remove_dir_all(&temp_dir);
}

#[test]
fn test_cli_mode_and_format_parsing() {
    // Test that aliases and various options are parsed successfully by clap
    let status_docs_md = Command::new("cargo")
        .args(["run", "--bin", "ctx", "--", ".", "-m", "docs", "-f", "md", "--no-stats"])
        .status()
        .expect("failed to execute cargo run");
    assert!(status_docs_md.success());

    let status_code_txt = Command::new("cargo")
        .args(["run", "--bin", "ctx", "--", ".", "-m", "code", "-f", "txt", "--no-stats"])
        .status()
        .expect("failed to execute cargo run");
    assert!(status_code_txt.success());

    let status_llm_text = Command::new("cargo")
        .args(["run", "--bin", "ctx", "--", ".", "-m", "llm", "-f", "text", "--no-stats"])
        .status()
        .expect("failed to execute cargo run");
    assert!(status_llm_text.success());

    // Test that invalid mode fails to parse
    let status_invalid_mode = Command::new("cargo")
        .args(["run", "--bin", "ctx", "--", ".", "-m", "invalid_mode"])
        .status()
        .expect("failed to execute cargo run");
    assert!(!status_invalid_mode.success());

    // Test that invalid format fails to parse
    let status_invalid_format = Command::new("cargo")
        .args(["run", "--bin", "ctx", "--", ".", "-f", "invalid_format"])
        .status()
        .expect("failed to execute cargo run");
    assert!(!status_invalid_format.success());
}

#[test]
fn test_cli_config_priority() {
    let temp_dir = std::env::temp_dir().join("ctx_cli_config_test");
    let _ = fs::remove_dir_all(&temp_dir);
    fs::create_dir_all(&temp_dir).unwrap();

    // Create a code file and a text doc file
    fs::write(temp_dir.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(temp_dir.join("doc.txt"), "some documentation\n").unwrap();

    // 1. CLI использует .ctxconfig (mode = docs), если аргументы не переданы
    fs::write(
        temp_dir.join(".ctxconfig"),
        "mode = docs\nexclude = excluded.txt\n",
    )
    .unwrap();

    let output_config_only = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "ctx",
            "--",
            temp_dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run ctx");
    
    assert!(output_config_only.status.success());
    let stdout_config_only = String::from_utf8(output_config_only.stdout).unwrap();
    // Under docs mode, doc.txt should be visible, but main.rs should be hidden
    assert!(stdout_config_only.contains("doc.txt"));
    assert!(!stdout_config_only.contains("main.rs"));

    // 2. CLI arguments переопределяют .ctxconfig (docs -> code)
    let output_override = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "ctx",
            "--",
            temp_dir.to_str().unwrap(),
            "-m",
            "code",
        ])
        .output()
        .expect("failed to run ctx");
    
    assert!(output_override.status.success());
    let stdout_override = String::from_utf8(output_override.stdout).unwrap();
    // Under code mode, main.rs should be visible, but doc.txt should be hidden
    assert!(stdout_override.contains("main.rs"));
    assert!(!stdout_override.contains("doc.txt"));

    // 3. некорректные значения в .ctxconfig обрабатываются предсказуемо (игнорируются, fallback к default = smart)
    fs::write(
        temp_dir.join(".ctxconfig"),
        "mode = invalid_mode_val\n",
    )
    .unwrap();

    let output_invalid = Command::new("cargo")
        .args([
            "run",
            "--bin",
            "ctx",
            "--",
            temp_dir.to_str().unwrap(),
        ])
        .output()
        .expect("failed to run ctx");
    
    assert!(output_invalid.status.success());
    let stdout_invalid = String::from_utf8(output_invalid.stdout).unwrap();
    // Default smart mode should keep both main.rs and doc.txt
    assert!(stdout_invalid.contains("main.rs"));
    assert!(stdout_invalid.contains("doc.txt"));

    let _ = fs::remove_dir_all(&temp_dir);
}
