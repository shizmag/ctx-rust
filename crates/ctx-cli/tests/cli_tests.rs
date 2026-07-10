use std::fs;
use std::path::PathBuf;

fn isolated_xdg_env(temp_root: &std::path::Path) -> PathBuf {
    let xdg = temp_root.join("xdg-config");
    fs::create_dir_all(&xdg).unwrap();
    xdg
}

#[test]
fn test_cli_help() {
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd.arg("--help").output().expect("failed to execute ctx");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("directory tree visualizer"));
}

#[test]
fn test_cli_scan() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();
    fs::write(temp_path.join("a.rs"), "fn main() {}\n").unwrap();

    // 1. Test ordinary call: should print colored tree and summary
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd.arg(temp_path).output().expect("failed to execute ctx");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    // Verify tree/summary containing folder and file details
    let temp_dir_name = temp_path.file_name().unwrap().to_str().unwrap();
    assert!(stdout.contains(temp_dir_name));
    assert!(stdout.contains("a.rs"));
    assert!(stdout.contains("Project Summary:"));

    // 2. Test call with -C (code): should print the full code content
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_code = cmd
        .args([temp_path.to_str().unwrap(), "--format", "plain", "-C"])
        .output()
        .expect("failed to execute ctx");

    assert!(output_code.status.success());
    let stdout_code = String::from_utf8(output_code.stdout).unwrap();
    assert!(stdout_code.contains("File: a.rs"));
    assert!(stdout_code.contains("fn main() {}"));
}

#[test]
fn test_cli_mode_and_format_parsing() {
    let temp_dir = tempfile::tempdir().unwrap();
    let path_str = temp_dir.path().to_str().unwrap();

    // Test that aliases and various options are parsed successfully by clap
    let status_docs_md = assert_cmd::Command::cargo_bin("ctx")
        .unwrap()
        .args([path_str, "-m", "docs", "-f", "md", "--no-stats"])
        .output()
        .expect("failed to execute ctx")
        .status;
    assert!(status_docs_md.success());

    let status_code_txt = assert_cmd::Command::cargo_bin("ctx")
        .unwrap()
        .args([path_str, "-m", "code", "-f", "txt", "--no-stats"])
        .output()
        .expect("failed to execute ctx")
        .status;
    assert!(status_code_txt.success());

    let status_llm_text = assert_cmd::Command::cargo_bin("ctx")
        .unwrap()
        .args([path_str, "-m", "llm", "-f", "text", "--no-stats"])
        .output()
        .expect("failed to execute ctx")
        .status;
    assert!(status_llm_text.success());

    // Test that invalid mode fails to parse
    let status_invalid_mode = assert_cmd::Command::cargo_bin("ctx")
        .unwrap()
        .args([path_str, "-m", "invalid_mode"])
        .output()
        .expect("failed to execute ctx")
        .status;
    assert!(!status_invalid_mode.success());

    // Test that invalid format fails to parse
    let status_invalid_format = assert_cmd::Command::cargo_bin("ctx")
        .unwrap()
        .args([path_str, "-f", "invalid_format"])
        .output()
        .expect("failed to execute ctx")
        .status;
    assert!(!status_invalid_format.success());
}

#[test]
fn test_cli_config_priority() {
    let temp_dir = tempfile::tempdir().unwrap();
    let temp_path = temp_dir.path();
    let xdg = isolated_xdg_env(temp_path);

    // Create a code file and a text doc file
    fs::write(temp_path.join("main.rs"), "fn main() {}\n").unwrap();
    fs::write(temp_path.join("doc.txt"), "some documentation\n").unwrap();

    // 1. CLI uses project .ctxconfig (mode = docs) when CLI args are not passed
    fs::write(
        temp_path.join(".ctxconfig"),
        "mode = docs\nexclude = excluded.txt\nformat = plain\n",
    )
    .unwrap();

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_config_only = cmd
        .env("XDG_CONFIG_HOME", &xdg)
        .args([temp_path.to_str().unwrap(), "-C", "--no-stats"])
        .output()
        .expect("failed to run ctx");

    assert!(output_config_only.status.success());
    let stdout_config_only = String::from_utf8(output_config_only.stdout).unwrap();
    // Under docs mode, doc.txt should be visible, but main.rs should be hidden
    assert!(stdout_config_only.contains("doc.txt"));
    assert!(!stdout_config_only.contains("main.rs"));
    // format=plain from config should produce plain render markers (not markdown #)
    assert!(stdout_config_only.contains("Project: "));
    assert!(!stdout_config_only.contains("# Project:"));

    // CLI fallback also works for parser alias agent_format (tied to default_format)
    fs::write(
        temp_path.join(".ctxconfig"),
        "mode = docs\nagent_format = plain\n",
    )
    .unwrap();

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_alias = cmd
        .env("XDG_CONFIG_HOME", &xdg)
        .args([temp_path.to_str().unwrap(), "-C", "--no-stats"])
        .output()
        .expect("failed to run ctx");

    assert!(output_alias.status.success());
    let stdout_alias = String::from_utf8(output_alias.stdout).unwrap();
    assert!(stdout_alias.contains("Project: "));
    assert!(!stdout_alias.contains("# Project:"));

    // 2. CLI arguments override .ctxconfig (docs -> code)
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_override = cmd
        .env("XDG_CONFIG_HOME", &xdg)
        .args([temp_path.to_str().unwrap(), "-m", "code"])
        .output()
        .expect("failed to run ctx");

    assert!(output_override.status.success());
    let stdout_override = String::from_utf8(output_override.stdout).unwrap();
    // Under code mode, main.rs should be visible, but doc.txt should be hidden
    assert!(stdout_override.contains("main.rs"));
    assert!(!stdout_override.contains("doc.txt"));

    // 3. invalid values in .ctxconfig are ignored with fallback to default = smart
    fs::write(temp_path.join(".ctxconfig"), "mode = invalid_mode_val\n").unwrap();

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_invalid = cmd
        .env("XDG_CONFIG_HOME", &xdg)
        .arg(temp_path)
        .output()
        .expect("failed to run ctx");

    assert!(output_invalid.status.success());
    let stdout_invalid = String::from_utf8(output_invalid.stdout).unwrap();
    // Default smart mode should keep both main.rs and doc.txt
    assert!(stdout_invalid.contains("main.rs"));
    assert!(stdout_invalid.contains("doc.txt"));
}

#[test]
fn test_cli_setting_subcommand_help_and_invocation() {
    // help works (non-interactive)
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd.args(["setting", "--help"]).output().expect("failed");
    assert!(output.status.success());
    let out = String::from_utf8(output.stdout).unwrap();
    assert!(out.contains("interactive TUI to view/edit"));
    assert!(out.contains("~/.config/ctx/config"));

    // invocation on temp dir returns error (TUI requires tty) but does not panic and reports via error path
    let temp_dir = tempfile::tempdir().unwrap();
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_run = cmd
        .args([temp_dir.path().to_str().unwrap(), "setting"])
        .output()
        .expect("failed to run");
    // exits non-zero, stderr mentions error or TUI
    assert!(!output_run.status.success());
    let stderr = String::from_utf8(output_run.stderr).unwrap_or_default();
    let stdout = String::from_utf8(output_run.stdout).unwrap_or_default();
    assert!(
        stderr.to_lowercase().contains("error")
            || stderr.to_lowercase().contains("raw")
            || stdout.to_lowercase().contains("error")
            || !stderr.is_empty()
    );
}

#[test]
fn test_cli_mcp_install_help_and_dry_run() {
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["mcp", "install", "--help"])
        .output()
        .expect("failed");
    assert!(output.status.success());
    let out = String::from_utf8(output.stdout).unwrap();
    assert!(out.contains("Auto-install") || out.contains("register the ctx MCP"));

    // dry-run should succeed without writing anything
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["mcp", "install", "--dry-run"])
        .output()
        .expect("failed");
    assert!(output.status.success());
    let out = String::from_utf8(output.stdout).unwrap();
    assert!(out.contains("dry-run") || out.contains("Would update"));
}
