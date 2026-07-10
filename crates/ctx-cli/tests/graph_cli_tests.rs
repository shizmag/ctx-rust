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
        pub fn a() {
            b();
        }

        fn b() {}

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
        .args([
            "graph",
            "build",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
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
        .args([
            "graph",
            "symbols",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
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
        .args([
            "graph",
            "calls",
            "run_pipeline",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
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
        .args([
            "graph",
            "callers",
            "load",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
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
        .args([
            "graph",
            "slice",
            "run_pipeline",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
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
        .args([
            "graph",
            "calls",
            "load",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
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

#[test]
fn test_cli_graph_help_and_alias() {
    // 1. Test ctx graph --help
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["graph", "--help"])
        .output()
        .expect("failed to run ctx graph --help");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Analyze the project and query dependency or symbol relationships"));
    assert!(stdout.contains("build"));
    assert!(stdout.contains("symbols"));
    assert!(stdout.contains("calls"));
    assert!(stdout.contains("callers"));
    assert!(stdout.contains("slice"));
    assert!(stdout.contains("info"));
    assert!(stdout.contains("Examples:"));
    assert!(stdout.contains("ctx g symbols"));
    assert!(stdout.contains("ctx g info"));

    // 2. Test ctx g --help
    let mut cmd_g = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_g = cmd_g
        .args(["g", "--help"])
        .output()
        .expect("failed to run ctx g --help");
    assert!(output_g.status.success());
    let stdout_g = String::from_utf8(output_g.stdout).unwrap();
    assert!(stdout_g.contains("Analyze the project and query dependency or symbol relationships"));
    assert!(stdout_g.contains("build"));
    assert!(stdout_g.contains("symbols"));
    assert!(stdout_g.contains("calls"));
    assert!(stdout_g.contains("callers"));
    assert!(stdout_g.contains("slice"));
    assert!(stdout_g.contains("info"));
    assert!(stdout_g.contains("Examples:"));
    assert!(stdout_g.contains("ctx g symbols"));
    assert!(stdout_g.contains("ctx g info"));
}

#[test]
fn test_cli_graph_info_before_and_after_build() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_missing = cmd
        .args(["graph", "info", root.to_str().unwrap()])
        .output()
        .expect("failed to run ctx graph info");
    assert!(output_missing.status.success());
    let stdout_missing = String::from_utf8(output_missing.stdout).unwrap();
    assert!(stdout_missing.contains("ctx graph info"));
    assert!(stdout_missing.contains("Workspace"));
    assert!(stdout_missing.contains("Index"));
    assert!(
        stdout_missing.contains("missing") || stdout_missing.contains("needs rebuild")
    );
    assert!(stdout_missing.contains("ctx graph build"));

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_build = cmd
        .args([
            "graph",
            "build",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed to run ctx graph build");
    assert!(output_build.status.success());

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_ready = cmd
        .args(["g", "info", root.to_str().unwrap()])
        .output()
        .expect("failed to run ctx g info");
    assert!(output_ready.status.success());
    let stdout_ready = String::from_utf8(output_ready.stdout).unwrap();
    assert!(stdout_ready.contains("ready") || stdout_ready.contains("stale"));
    assert!(stdout_ready.contains("symbols:"));
    assert!(stdout_ready.contains("rust") || stdout_ready.contains("files:"));

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_json = cmd
        .args([
            "graph",
            "info",
            root.to_str().unwrap(),
            "--format",
            "json",
        ])
        .output()
        .expect("failed to run ctx graph info --format json");
    assert!(output_json.status.success());
    let stdout_json = String::from_utf8(output_json.stdout).unwrap();
    assert!(stdout_json.contains("\"workspace_root\""));
    assert!(stdout_json.contains("\"symbols\""));
    assert!(stdout_json.contains("\"state\""));
}

#[test]
fn test_cli_g_alias_execution() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    // Verify symbols output via 'g' alias
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args(["g", "symbols", root.to_str().unwrap(), "--no-rust-analyzer"])
        .output()
        .expect("failed to run ctx g symbols");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("run_pipeline"));
    assert!(stdout.contains("load"));
    assert!(stdout.contains("process"));
}

#[test]
fn test_cli_graph_context_happy_case() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "context",
            "a",
            "--mode",
            "callees",
            "--depth",
            "2",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed to run ctx graph context");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();

    // Check output contains '# Graph Context'
    assert!(stdout.contains("# Graph Context"), "Stdout: {}", stdout);

    // Check output contains root symbol
    assert!(stdout.contains("Root: fn a"), "Stdout: {}", stdout);

    // Check output contains included symbols
    assert!(stdout.contains("- fn b"), "Stdout: {}", stdout);
}

#[test]
fn test_cli_graph_context_ambiguous() {
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
            pub fn ambig() {}
        }
        pub mod m2 {
            pub fn ambig() {}
        }
    "#;
    fs::write(src_dir.join("lib.rs"), lib_code).unwrap();

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "context",
            "ambig",
            "--mode",
            "callees",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed to run ctx graph context ambig");

    // It should exit with code 1 (failure)
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Ambiguous symbol: ambig"));
    assert!(stderr.contains("Candidates:"));
    assert!(stderr.contains("m1::ambig"));
    assert!(stderr.contains("m2::ambig"));
}

#[test]
fn test_cli_graph_affect() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    // 1. Text mode
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "affect",
            "run_pipeline",
            "--token-budget",
            "12000",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed to run ctx graph affect");

    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("run_pipeline"));
    assert!(stdout.contains("load"));
    assert!(stdout.contains("process"));

    // 2. JSON mode
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output_json = cmd
        .args([
            "graph",
            "affect",
            "run_pipeline",
            "--token-budget",
            "12000",
            "--format",
            "json",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed to run ctx graph affect json");

    assert!(output_json.status.success());
    let stdout_json = String::from_utf8(output_json.stdout).unwrap();
    let val: serde_json::Value = serde_json::from_str(&stdout_json).unwrap();
    assert_eq!(val["query"], "run_pipeline");
    assert!(val["token_budget"].as_u64().is_some());
    assert!(val["roots"].as_array().unwrap().len() >= 1);
}

#[test]
fn test_cli_graph_affect_failures() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    // 1. Conflict check: with-snippets and no-snippets together
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "affect",
            "run_pipeline",
            "--with-snippets",
            "--no-snippets",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("cannot be used with"));

    // 2. Invalid format check
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "affect",
            "run_pipeline",
            "--format",
            "unknown_fmt",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Invalid format"));

    // 3. Invalid depth check
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "affect",
            "run_pipeline",
            "--depth",
            "nope",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .unwrap();
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Invalid depth"));
}

/// Regression test for unified index lifecycle: after initial build, query
/// subcommands on Ready index produce no "update"/"built" side-effect messages.
#[test]
fn test_cli_graph_queries_no_output_on_fresh_index() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    // First, ensure index via explicit build (non-verbose)
    let mut build_cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let build_out = build_cmd
        .args([
            "graph",
            "build",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("build failed");
    assert!(build_out.status.success());

    // Now run a query subcommand; should succeed with symbol data, no build msgs
    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "symbols",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("symbols query failed");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("run_pipeline") || stdout.contains("a"));
    // No incidental build/update notifications when Ready
    assert!(
        !stdout.contains("Incremental update") &&
        !stdout.contains("Built codegraph") &&
        !stdout.contains("Index not found"),
        "unexpected build noise on fresh index: {}", stdout
    );

    // Also test calls path
    let mut cmd2 = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let out2 = cmd2
        .args([
            "graph",
            "calls",
            "run_pipeline",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("calls failed");
    assert!(out2.status.success());
    let s2 = String::from_utf8(out2.stdout).unwrap();
    assert!(!s2.contains("Incremental update"));
}

#[test]
fn test_cli_graph_symbols_query_unique_and_not_found() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "symbols",
            "run_pipeline",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed unique symbol query");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Unique match:"));
    assert!(stdout.contains("run_pipeline"));

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "symbols",
            "definitely_missing_symbol",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed not-found symbol query");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Symbol not found"));
}

#[test]
fn test_cli_graph_callees_alias_and_build_verbose() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "callees",
            "run_pipeline",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed callees alias");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Callees of"));
    assert!(stdout.contains("run_pipeline"));
    assert!(stdout.contains("load"));

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "build",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
            "--verbose",
        ])
        .output()
        .expect("failed verbose build");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Codegraph Build Report"));
}

#[test]
fn test_cli_graph_callers_not_found() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "callers",
            "missing_symbol_xyz",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed graph callers not found");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Symbol not found"));
}

#[test]
fn test_cli_graph_slice_not_found() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "slice",
            "no_such_fn",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed graph slice not found");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Symbol not found"));
}

#[test]
fn test_cli_graph_calls_not_found() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "calls",
            "ghost_fn",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed graph calls not found");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Symbol not found"));
}

#[test]
fn test_cli_graph_symbols_ambiguous_query() {
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
    fs::write(
        src_dir.join("lib.rs"),
        r#"
        pub mod m1 { pub fn dup() {} }
        pub mod m2 { pub fn dup() {} }
        "#,
    )
    .unwrap();

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "symbols",
            "dup",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed ambiguous symbols query");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("Ambiguous query"));
    assert!(stdout.contains("m1::dup"));
    assert!(stdout.contains("m2::dup"));
}

#[test]
fn test_cli_graph_context_not_found() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "context",
            "missing_ctx_symbol",
            "--mode",
            "callees",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed graph context not found");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("Symbol not found"));
}

#[test]
fn test_cli_graph_symbols_dir_as_query_path() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "symbols",
            root.join("src").to_str().unwrap(),
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed graph symbols with dir query");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(stdout.contains("lib.rs"));
    assert!(stdout.contains("run_pipeline") || stdout.contains("a"));
}

#[test]
fn test_cli_graph_affect_text_mode_with_snippets() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "affect",
            "run_pipeline",
            "--format",
            "text",
            "--with-snippets",
            "--mode",
            "callees",
            root.to_str().unwrap(),
            "--no-rust-analyzer",
        ])
        .output()
        .expect("failed graph affect text");
    assert!(output.status.success());
    let stdout = String::from_utf8(output.stdout).unwrap();
    assert!(!stdout.is_empty());
}

#[test]
fn test_cli_graph_info_invalid_format() {
    let temp_dir = tempfile::tempdir().unwrap();
    let root = temp_dir.path();
    create_temp_project(root);

    let mut cmd = assert_cmd::Command::cargo_bin("ctx").unwrap();
    let output = cmd
        .args([
            "graph",
            "info",
            root.to_str().unwrap(),
            "--format",
            "yaml",
        ])
        .output()
        .expect("failed graph info invalid format");
    assert!(!output.status.success());
    let stderr = String::from_utf8(output.stderr).unwrap();
    assert!(stderr.contains("unsupported format"));
}
