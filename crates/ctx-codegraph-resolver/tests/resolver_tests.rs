use ctx_codegraph_lang::backend::{BackendId, ResolveInput, ResolverBackend};
use ctx_codegraph_lang::model::{
    LanguageId, Occurrence, OccurrenceKind, ResolutionConfidence, Symbol, SymbolKind, TextRange,
};
use ctx_codegraph_resolver::LspDefinitionResolver;
use std::fs::{self, File};
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::sync::Mutex;
use tempfile::tempdir;

static ENV_MUTEX: Mutex<()> = Mutex::new(());

fn env_lock() -> std::sync::MutexGuard<'static, ()> {
    ENV_MUTEX
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

#[test]
fn test_python_resolver_mock_lsp() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let script_path = bin_dir.join("pyright-langserver");
    let script_content = r#"#!/usr/bin/env python3
import sys
import json
import traceback

log_file = open("/tmp/mock_pyright.log", "w")

def log(msg):
    log_file.write(f"{msg}\n")
    log_file.flush()

def write_response(id, result):
    response = {
        "jsonrpc": "2.0",
        "id": id,
        "result": result
    }
    msg = json.dumps(response)
    log(f"Writing response: {msg}")
    sys.stdout.write(f"Content-Length: {len(msg)}\r\n\r\n{msg}")
    sys.stdout.flush()

log("Script started")
try:
    while True:
        line = sys.stdin.readline()
        if not line:
            log("No more input (EOF)")
            break
        log(f"Read line: {repr(line)}")
        if line.startswith("Content-Length:"):
            length = int(line.split(":")[1].strip())
            empty = sys.stdin.readline()
            log(f"Read empty line: {repr(empty)}")
            content = sys.stdin.read(length)
            log(f"Read content: {content}")
            req = json.loads(content)
            method = req.get("method")
            req_id = req.get("id")
            log(f"Method: {method}, id: {req_id}")
            
            if method == "initialize":
                write_response(req_id, {"capabilities": {"textDocumentSync": 1}})
            elif method == "textDocument/definition":
                uri = req.get("params", {}).get("textDocument", {}).get("uri")
                if uri:
                    write_response(req_id, [
                        {
                            "uri": uri,
                            "range": {
                                "start": {"line": 4, "character": 4},
                                "end": {"line": 4, "character": 8}
                            }
                        }
                    ])
                else:
                    write_response(req_id, [])
except Exception as e:
    log(f"Exception: {e}")
    log(traceback.format_exc())
"#;

    {
        let mut file = File::create(&script_path).unwrap();
        file.write_all(script_content.as_bytes()).unwrap();
    }

    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    unsafe {
        std::env::set_var("PATH", &new_path);
    }

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# Dummy python file\n").unwrap();

    let occurrence = Occurrence {
        id: None,
        file_id: None,
        enclosing_symbol: None,
        enclosing_temp_index: None,
        kind: OccurrenceKind::Call,
        raw_text: "pipeline.run".to_string(),
        file: test_file.clone(),
        range: TextRange {
            start_line: 14,
            start_col: 18,
            end_line: 14,
            end_col: 30,
        },
        language: LanguageId::new("python"),
        backend_id: BackendId::new("python-backend"),
    };

    let symbols = vec![Symbol {
        id: None,
        file_id: None,
        name: "run".to_string(),
        qualified_name: "RAGPipeline::run".to_string(),
        kind: SymbolKind::Method,
        language: LanguageId::new("python"),
        file: test_file.clone(),
        range: TextRange {
            start_line: 5,
            start_col: 5,
            end_line: 5,
            end_col: 10,
        },
        body_range: None,
    }];

    let resolver = LspDefinitionResolver::python();
    let input = ResolveInput {
        workspace_root,
        occurrence: &occurrence,
        symbols: &symbols,
    };

    let output = resolver.resolve(input).unwrap();

    unsafe {
        std::env::set_var("PATH", old_path);
    }

    assert_eq!(output.resolved_symbol_index, Some(0));
    assert_eq!(output.confidence, ResolutionConfidence::LspExact);
}

#[test]
fn test_python_resolver_fallback_noop() {
    let _guard = env_lock();
    let old_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", "");
    }

    let temp_dir = tempdir().unwrap();
    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# Dummy python file\n").unwrap();

    let occurrence = Occurrence {
        id: None,
        file_id: None,
        enclosing_symbol: None,
        enclosing_temp_index: None,
        kind: OccurrenceKind::Call,
        raw_text: "run".to_string(),
        file: test_file.clone(),
        range: TextRange {
            start_line: 14,
            start_col: 18,
            end_line: 14,
            end_col: 21,
        },
        language: LanguageId::new("python"),
        backend_id: BackendId::new("python-backend"),
    };

    let symbols = vec![Symbol {
        id: None,
        file_id: None,
        name: "run".to_string(),
        qualified_name: "RAGPipeline::run".to_string(),
        kind: SymbolKind::Method,
        language: LanguageId::new("python"),
        file: test_file.clone(),
        range: TextRange {
            start_line: 5,
            start_col: 5,
            end_line: 5,
            end_col: 10,
        },
        body_range: None,
    }];

    let resolver = LspDefinitionResolver::python();
    let input = ResolveInput {
        workspace_root,
        occurrence: &occurrence,
        symbols: &symbols,
    };

    let output = resolver.resolve(input).unwrap();

    unsafe {
        std::env::set_var("PATH", old_path);
    }

    assert_eq!(output.resolved_symbol_index, Some(0));
    assert_eq!(output.confidence, ResolutionConfidence::Syntax);
}

fn write_custom_mock_lsp_script(bin_dir: &std::path::Path, command_name: &str, definition_handler: &str) {
    let script_path = bin_dir.join(command_name);
    let script_content = [
        "#!/usr/bin/env python3",
        "import sys",
        "import json",
        "",
        "def write_response(req_id, result):",
        "    response = {\"jsonrpc\": \"2.0\", \"id\": req_id, \"result\": result}",
        "    msg = json.dumps(response)",
        "    sys.stdout.write(f\"Content-Length: {len(msg)}\\r\\n\\r\\n{msg}\")",
        "    sys.stdout.flush()",
        "",
        "def write_error(req_id, code, message):",
        "    response = {\"jsonrpc\": \"2.0\", \"id\": req_id, \"error\": {\"code\": code, \"message\": message}}",
        "    msg = json.dumps(response)",
        "    sys.stdout.write(f\"Content-Length: {len(msg)}\\r\\n\\r\\n{msg}\")",
        "    sys.stdout.flush()",
        "",
        "while True:",
        "    line = sys.stdin.readline()",
        "    if not line:",
        "        break",
        "    if line.startswith(\"Content-Length:\"):",
        "        length = int(line.split(\":\")[1].strip())",
        "        sys.stdin.readline()",
        "        content = sys.stdin.read(length)",
        "        req = json.loads(content)",
        "        method = req.get(\"method\")",
        "        req_id = req.get(\"id\")",
        "        if method == \"initialize\":",
        "            write_response(req_id, {\"capabilities\": {\"textDocumentSync\": 1}})",
        "        elif method == \"textDocument/definition\":",
        definition_handler,
        "",
    ]
    .join("\n");

    let mut file = File::create(&script_path).unwrap();
    file.write_all(script_content.as_bytes()).unwrap();

    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();
}

fn write_mock_lsp_script(bin_dir: &std::path::Path, command_name: &str, definition_mode: &str) {
    let script_path = bin_dir.join(command_name);
    let definition_handler = match definition_mode {
        "standard" => r#"                write_response(req_id, [{
                    "uri": uri,
                    "range": {
                        "start": {"line": 4, "character": 4},
                        "end": {"line": 4, "character": 8}
                    }
                }])"#,
        "extended" => r#"                write_response(req_id, [{
                    "targetUri": uri,
                    "targetRange": {
                        "start": {"line": 4, "character": 4},
                        "end": {"line": 4, "character": 8}
                    }
                }])"#,
        "object" => r#"                write_response(req_id, {
                    "uri": uri,
                    "range": {
                        "start": {"line": 4, "character": 4},
                        "end": {"line": 4, "character": 8}
                    }
                })"#,
        _ => panic!("unknown definition_mode: {definition_mode}"),
    };

    let script_content = [
        "#!/usr/bin/env python3",
        "import sys",
        "import json",
        "",
        "def write_response(req_id, result):",
        "    response = {\"jsonrpc\": \"2.0\", \"id\": req_id, \"result\": result}",
        "    msg = json.dumps(response)",
        "    sys.stdout.write(f\"Content-Length: {len(msg)}\\r\\n\\r\\n{msg}\")",
        "    sys.stdout.flush()",
        "",
        "while True:",
        "    line = sys.stdin.readline()",
        "    if not line:",
        "        break",
        "    if line.startswith(\"Content-Length:\"):",
        "        length = int(line.split(\":\")[1].strip())",
        "        sys.stdin.readline()",
        "        content = sys.stdin.read(length)",
        "        req = json.loads(content)",
        "        method = req.get(\"method\")",
        "        req_id = req.get(\"id\")",
        "        if method == \"initialize\":",
        "            write_response(req_id, {\"capabilities\": {\"textDocumentSync\": 1}})",
        "        elif method == \"textDocument/definition\":",
        "            uri = req.get(\"params\", {}).get(\"textDocument\", {}).get(\"uri\")",
        "            if uri:",
        definition_handler,
        "            else:",
        "                write_response(req_id, [])",
        "",
    ]
    .join("\n");

    let mut file = File::create(&script_path).unwrap();
    file.write_all(script_content.as_bytes()).unwrap();

    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();
}

fn with_mock_path<F>(bin_dir: &std::path::Path, f: F)
where
    F: FnOnce(),
{
    let old_path = std::env::var("PATH").unwrap_or_default();
    let new_path = format!("{}:{}", bin_dir.display(), old_path);
    unsafe {
        std::env::set_var("PATH", &new_path);
    }
    f();
    unsafe {
        std::env::set_var("PATH", old_path);
    }
}

fn sample_occurrence(file: &std::path::Path) -> Occurrence {
    Occurrence {
        id: None,
        file_id: None,
        enclosing_symbol: None,
        enclosing_temp_index: None,
        kind: OccurrenceKind::Call,
        raw_text: "run".to_string(),
        file: file.to_path_buf(),
        range: TextRange {
            start_line: 14,
            start_col: 18,
            end_line: 14,
            end_col: 21,
        },
        language: LanguageId::new("python"),
        backend_id: BackendId::new("python-backend"),
    }
}

fn sample_symbols(file: &std::path::Path) -> Vec<Symbol> {
    vec![Symbol {
        id: None,
        file_id: None,
        name: "run".to_string(),
        qualified_name: "RAGPipeline::run".to_string(),
        kind: SymbolKind::Method,
        language: LanguageId::new("python"),
        file: file.to_path_buf(),
        range: TextRange {
            start_line: 5,
            start_col: 5,
            end_line: 5,
            end_col: 10,
        },
        body_range: None,
    }]
}

#[test]
fn test_resolver_metadata_and_constructors() {
    let _guard = env_lock();
    let rust = LspDefinitionResolver::rust();
    assert_eq!(rust.resolver_id().0, "rust-analyzer-lsp");
    assert_eq!(rust.resolver_version(), "0.1.0");

    let python = LspDefinitionResolver::python();
    assert_eq!(python.resolver_id().0, "pyright-lsp");
    assert_eq!(python.resolver_version(), "0.1.0");
}

#[test]
fn test_rust_resolver_mock_lsp_standard_location() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_mock_lsp_script(&bin_dir, "rust-analyzer", "standard");

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.rs");
    fs::write(&test_file, "fn main() {}\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::rust();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::LspExact);
    });
}

#[test]
fn test_python_resolver_extended_location_fields() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_mock_lsp_script(&bin_dir, "pyright-langserver", "extended");

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::LspExact);
    });
}

#[test]
fn test_lsp_single_object_location_response() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_mock_lsp_script(&bin_dir, "pyright-langserver", "object");

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::LspExact);
    });
}

#[test]
fn test_lsp_workspace_change_recreates_client() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_mock_lsp_script(&bin_dir, "pyright-langserver", "standard");

    let workspace_a = temp_dir.path().join("a");
    let workspace_b = temp_dir.path().join("b");
    fs::create_dir_all(&workspace_a).unwrap();
    fs::create_dir_all(&workspace_b).unwrap();
    let file_a = workspace_a.join("main.py");
    let file_b = workspace_b.join("main.py");
    fs::write(&file_a, "# a\n").unwrap();
    fs::write(&file_b, "# b\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();

        let input_a = ResolveInput {
            workspace_root: &workspace_a,
            occurrence: &sample_occurrence(&file_a),
            symbols: &sample_symbols(&file_a),
        };
        let out_a = resolver.resolve(input_a).unwrap();
        assert_eq!(out_a.confidence, ResolutionConfidence::LspExact);

        let input_b = ResolveInput {
            workspace_root: &workspace_b,
            occurrence: &sample_occurrence(&file_b),
            symbols: &sample_symbols(&file_b),
        };
        let out_b = resolver.resolve(input_b).unwrap();
        assert_eq!(out_b.confidence, ResolutionConfidence::LspExact);
    });
}

#[test]
fn test_lsp_transport_spawn_failure() {
    let _guard = env_lock();
    let old_path = std::env::var("PATH").unwrap_or_default();
    unsafe {
        std::env::set_var("PATH", "");
    }

    let temp_dir = tempdir().unwrap();
    let result = ctx_codegraph_resolver::GenericLspClient::new(
        temp_dir.path(),
        "definitely-not-a-real-lsp-binary",
        &[],
    );
    unsafe {
        std::env::set_var("PATH", old_path);
    }

    let err = result.err().expect("expected spawn failure");
    assert!(err.contains("Failed to spawn") || err.contains("definitely-not-a-real-lsp-binary"));
}

#[test]
fn test_lsp_empty_definition_falls_back_to_name_match() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_custom_mock_lsp_script(
        &bin_dir,
        "pyright-langserver",
        r#"            uri = req.get("params", {}).get("textDocument", {}).get("uri")
            if uri:
                write_response(req_id, [])
            else:
                write_response(req_id, [])"#,
    );

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::Syntax);
    });
}

#[test]
fn test_lsp_definition_error_falls_back_with_warning() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_custom_mock_lsp_script(
        &bin_dir,
        "pyright-langserver",
        r#"            write_error(req_id, -32000, "definition unavailable")"#,
    );

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::Syntax);
    });
}

#[test]
fn test_lsp_retries_transient_32603_errors_during_warmup() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_custom_mock_lsp_script(
        &bin_dir,
        "pyright-langserver",
        r#"            global definition_calls
            try:
                definition_calls
            except NameError:
                definition_calls = 0
            definition_calls += 1
            uri = req.get("params", {}).get("textDocument", {}).get("uri")
            if definition_calls < 2:
                write_error(req_id, -32603, "file not found yet")
            elif uri:
                write_response(req_id, [{
                    "targetUri": uri,
                    "targetRange": {
                        "start": {"line": 4, "character": 4},
                        "end": {"line": 4, "character": 8}
                    }
                }])
            else:
                write_response(req_id, [])"#,
    );

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::LspExact);
    });
}

#[test]
fn test_lsp_non_file_uri_location_falls_back() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_custom_mock_lsp_script(
        &bin_dir,
        "pyright-langserver",
        r#"            write_response(req_id, [{
                "uri": "https://example.com/main.py",
                "range": {
                    "start": {"line": 4, "character": 4},
                    "end": {"line": 4, "character": 8}
                }
            }])"#,
    );

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::Syntax);
    });
}

#[test]
fn test_lsp_null_definition_result_falls_back() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_custom_mock_lsp_script(
        &bin_dir,
        "pyright-langserver",
        r#"            write_response(req_id, None)"#,
    );

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &sample_symbols(&test_file),
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::Syntax);
    });
}

#[test]
fn test_lsp_no_matching_symbol_at_location_falls_back() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    write_mock_lsp_script(&bin_dir, "pyright-langserver", "standard");

    let workspace_root = temp_dir.path();
    let test_file = workspace_root.join("main.py");
    fs::write(&test_file, "# python\n").unwrap();

    // Symbol range does not overlap LSP target line 5 / col 5 from mock.
    let symbols = vec![Symbol {
        id: None,
        file_id: None,
        name: "run".to_string(),
        qualified_name: "Other::run".to_string(),
        kind: SymbolKind::Method,
        language: LanguageId::new("python"),
        file: test_file.clone(),
        range: TextRange {
            start_line: 20,
            start_col: 1,
            end_line: 20,
            end_col: 10,
        },
        body_range: None,
    }];

    with_mock_path(&bin_dir, || {
        let resolver = LspDefinitionResolver::python();
        let input = ResolveInput {
            workspace_root,
            occurrence: &sample_occurrence(&test_file),
            symbols: &symbols,
        };
        let output = resolver.resolve(input).unwrap();
        assert_eq!(output.resolved_symbol_index, Some(0));
        assert_eq!(output.confidence, ResolutionConfidence::Syntax);
    });
}

#[test]
fn test_lsp_transport_timeout_on_non_responsive_server() {
    let _guard = env_lock();
    let temp_dir = tempdir().unwrap();
    let bin_dir = temp_dir.path().join("bin");
    fs::create_dir_all(&bin_dir).unwrap();

    let script_path = bin_dir.join("slow-langserver");
    let script_content = r#"#!/usr/bin/env python3
import sys
# Read initialize but never respond
while True:
    line = sys.stdin.readline()
    if not line:
        break
    if line.startswith("Content-Length:"):
        length = int(line.split(":")[1].strip())
        sys.stdin.readline()
        sys.stdin.read(length)
"#;
    let mut file = File::create(&script_path).unwrap();
    file.write_all(script_content.as_bytes()).unwrap();
    let mut perms = fs::metadata(&script_path).unwrap().permissions();
    perms.set_mode(0o755);
    fs::set_permissions(&script_path, perms).unwrap();

    with_mock_path(&bin_dir, || {
        let result = ctx_codegraph_resolver::GenericLspClient::new(
            temp_dir.path(),
            "slow-langserver",
            &[],
        );
        let err = result.err().expect("expected initialize timeout");
        assert!(err.contains("Timeout") || err.contains("initialize"));
    });
}